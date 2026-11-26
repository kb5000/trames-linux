// SPDX-License-Identifier: GPL-3.0-or-later
//! Native Vulkan scene composition. GPU planes are sampled from their original
//! image/device; only SHM planes upload pixels. All queue calls use its retained
//! synchronization capability and frame leases survive the submission fence.
use ash::{vk, vk::Handle};
use std::{
    io,
    sync::{Arc, OnceLock},
};
use trames_graphics::{NativeDevice, NativeHandle, VulkanQueue};
use trames_wayland::{ImageBuffer, WindowSnapshot};
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("Vulkan desktop: {e}"))
}
#[derive(Clone, Copy)]
pub enum Target {
    Xcb { connection: usize, window: u32 },
    Wayland { display: usize, surface: usize },
}
struct Owner {
    entry: ash::Entry,
    instance: ash::Instance,
    device: OnceLock<ash::Device>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        unsafe {
            if let Some(d) = self.device.get() {
                let _ = d.device_wait_idle();
                d.destroy_device(None);
            }
            self.instance.destroy_instance(None);
        }
    }
}
struct Surface {
    loader: ash::khr::surface::Instance,
    raw: vk::SurfaceKHR,
    _owner: Arc<dyn Send + Sync>,
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            self.loader.destroy_surface(self.raw, None);
        }
    }
}
struct Context {
    _entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
    physical: vk::PhysicalDevice,
    queue: VulkanQueue,
}
impl Context {
    unsafe fn surface(
        entry: &ash::Entry,
        instance: &ash::Instance,
        target: Target,
        owner: Arc<dyn Send + Sync>,
    ) -> io::Result<Surface> {
        let raw = match target {
            Target::Xcb { connection, window } => unsafe {
                ash::khr::xcb_surface::Instance::new(entry, instance).create_xcb_surface(
                    &vk::XcbSurfaceCreateInfoKHR::default()
                        .connection(connection as *mut _)
                        .window(window),
                    None,
                )
            },
            Target::Wayland { display, surface } => unsafe {
                ash::khr::wayland_surface::Instance::new(entry, instance).create_wayland_surface(
                    &vk::WaylandSurfaceCreateInfoKHR::default()
                        .display(display as *mut _)
                        .surface(surface as *mut _),
                    None,
                )
            },
        }
        .map_err(error)?;
        Ok(Surface {
            loader: ash::khr::surface::Instance::new(entry, instance),
            raw,
            _owner: owner,
        })
    }
    unsafe fn open(target: Target, scene: &WindowSnapshot) -> io::Result<(Self, Surface)> {
        let entry = unsafe { ash::Entry::load() }.map_err(error)?;
        if let Some(image) = scene.scene.iter().find_map(|s| {
            if let ImageBuffer::Gpu(g) = &s.buffer {
                Some(g)
            } else {
                None
            }
        }) {
            let q = image
                .vulkan_queue()
                .ok_or_else(|| error("image has no native queue capability"))?;
            if !match target {
                Target::Xcb { .. } => q.supports_xcb(),
                Target::Wayland { .. } => q.supports_wayland(),
            } {
                return Err(error(
                    "producer did not enable the native surface/swapchain extensions",
                ));
            }
            let NativeDevice::Vulkan {
                instance,
                physical,
                device,
            } = q.device()
            else {
                return Err(error("non-Vulkan device"));
            };
            let instance =
                unsafe { ash::Instance::load(entry.static_fn(), vk::Instance::from_raw(instance)) };
            let device =
                unsafe { ash::Device::load(instance.fp_v1_0(), vk::Device::from_raw(device)) };
            let surface = unsafe { Self::surface(&entry, &instance, target, Arc::new(q.clone())) }?;
            return Ok((
                Self {
                    _entry: entry,
                    instance,
                    device,
                    physical: vk::PhysicalDevice::from_raw(physical),
                    queue: q,
                },
                surface,
            ));
        }
        let ext = match target {
            Target::Xcb { .. } => ash::khr::xcb_surface::NAME,
            Target::Wayland { .. } => ash::khr::wayland_surface::NAME,
        };
        let names = [ash::khr::surface::NAME.as_ptr(), ext.as_ptr()];
        let app = vk::ApplicationInfo::default()
            .application_name(c"Trames desktop")
            .api_version(vk::API_VERSION_1_0);
        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(&app)
                    .enabled_extension_names(&names),
                None,
            )
        }
        .map_err(error)?;
        let owner = Arc::new(Owner {
            entry,
            instance,
            device: OnceLock::new(),
        });
        let surface =
            unsafe { Self::surface(&owner.entry, &owner.instance, target, owner.clone()) }?;
        let mut devices = unsafe { owner.instance.enumerate_physical_devices() }.map_err(error)?;
        devices.sort_by_key(|p| {
            let info = unsafe { owner.instance.get_physical_device_properties(*p) };
            match info.device_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => 0,
                vk::PhysicalDeviceType::INTEGRATED_GPU => 1,
                _ => 2,
            }
        });
        let (physical, family) = devices
            .into_iter()
            .find_map(|p| {
                unsafe {
                    owner
                        .instance
                        .get_physical_device_queue_family_properties(p)
                }
                .iter()
                .enumerate()
                .find_map(|(i, q)| {
                    (q.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                        && unsafe {
                            surface.loader.get_physical_device_surface_support(
                                p,
                                i as u32,
                                surface.raw,
                            )
                        }
                        .unwrap_or(false))
                    .then_some((p, i as u32))
                })
            })
            .ok_or_else(|| error("no graphics/presentation queue"))?;
        let priorities = [1.];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&priorities)];
        let extensions = [ash::khr::swapchain::NAME.as_ptr()];
        let device = unsafe {
            owner.instance.create_device(
                physical,
                &vk::DeviceCreateInfo::default()
                    .queue_create_infos(&queues)
                    .enabled_extension_names(&extensions),
                None,
            )
        }
        .map_err(error)?;
        owner
            .device
            .set(device.clone())
            .map_err(|_| error("device already initialized"))?;
        let raw = unsafe { device.get_device_queue(family, 0) };
        let q = unsafe {
            VulkanQueue::retained(
                NativeDevice::Vulkan {
                    instance: owner.instance.handle().as_raw(),
                    physical: physical.as_raw(),
                    device: device.handle().as_raw(),
                },
                family,
                raw.as_raw(),
                match target {
                    Target::Xcb { .. } => 1,
                    Target::Wayland { .. } => 2,
                },
                Arc::new(std::sync::Mutex::new(())),
                owner.clone(),
            )
        };
        // The entry dispatch is independently retained by this context; owner keeps
        // the original loader/instance/device until all native capabilities drop.
        let entry = unsafe { ash::Entry::load() }.map_err(error)?;
        Ok((
            Self {
                _entry: entry,
                instance: owner.instance.clone(),
                device,
                physical,
                queue: q,
            },
            surface,
        ))
    }
    fn memory(&self, bits: u32, flags: vk::MemoryPropertyFlags) -> io::Result<u32> {
        let p = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.physical)
        };
        (0..p.memory_type_count)
            .find(|i| {
                bits & (1 << i) != 0 && p.memory_types[*i as usize].property_flags.contains(flags)
            })
            .ok_or_else(|| error("no compatible image memory"))
    }
}
struct Frame {
    device: ash::Device,
    pool: vk::CommandPool,
    fences: Vec<vk::Fence>,
    framebuffers: Vec<vk::Framebuffer>,
    views: Vec<vk::ImageView>,
    images: Vec<vk::Image>,
    buffers: Vec<vk::Buffer>,
    memory: Vec<vk::DeviceMemory>,
    descriptors: vk::DescriptorPool,
}
impl Frame {
    fn new(device: &ash::Device) -> Self {
        Self {
            device: device.clone(),
            pool: vk::CommandPool::null(),
            fences: vec![],
            framebuffers: vec![],
            views: vec![],
            images: vec![],
            buffers: vec![],
            memory: vec![],
            descriptors: vk::DescriptorPool::null(),
        }
    }
    fn fence(&mut self) -> io::Result<vk::Fence> {
        let f = unsafe {
            self.device
                .create_fence(&vk::FenceCreateInfo::default(), None)
        }
        .map_err(error)?;
        self.fences.push(f);
        Ok(f)
    }
    fn view(&mut self, image: vk::Image, format: vk::Format) -> io::Result<vk::ImageView> {
        let v = unsafe {
            self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .level_count(1)
                            .layer_count(1),
                    ),
                None,
            )
        }
        .map_err(error)?;
        self.views.push(v);
        Ok(v)
    }
    fn texture(
        &mut self,
        c: &Context,
        cmd: vk::CommandBuffer,
        plane: &trames_wayland::SurfaceImage,
    ) -> io::Result<(vk::ImageView, vk::ImageLayout, bool)> {
        let d = &c.device;
        if let ImageBuffer::Gpu(g) = &plane.buffer {
            let q = g
                .vulkan_queue()
                .ok_or_else(|| error("GPU plane has no retained queue"))?;
            if q.device() != c.queue.device() || q.raw() != c.queue.raw() {
                return Err(error(
                    "scene GPU planes must share a native device and queue",
                ));
            }
            let NativeHandle::VulkanImage(info) = g.native_handle() else {
                return Err(error("unsupported native image kind"));
            };
            let format = vk::Format::from_raw(info.format as i32);
            if ![
                vk::Format::B8G8R8A8_UNORM,
                vk::Format::R8G8B8A8_UNORM,
                vk::Format::B8G8R8A8_SRGB,
                vk::Format::R8G8B8A8_SRGB,
            ]
            .contains(&format)
            {
                return Err(error("unsupported scene image format"));
            }
            let image = vk::Image::from_raw(info.image);
            let layout = vk::ImageLayout::from_raw(info.layout as i32);
            let barrier = vk::ImageMemoryBarrier::default()
                .image(image)
                .old_layout(layout)
                .new_layout(layout)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .level_count(1)
                        .layer_count(1),
                );
            unsafe {
                d.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
            }
            return Ok((
                self.view(image, format)?,
                layout,
                [vk::Format::B8G8R8A8_SRGB, vk::Format::R8G8B8A8_SRGB].contains(&format),
            ));
        }
        let shm = plane.buffer.shm().unwrap();
        let length = shm.width as u64 * shm.height as u64 * 4;
        if shm.width == 0
            || shm.height == 0
            || shm.stride < shm.width * 4
            || (shm.height as usize - 1) * shm.stride as usize + shm.width as usize * 4
                > shm.mapping.len()
        {
            return Err(error("invalid retained SHM range"));
        }
        let buffer = unsafe {
            d.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(length)
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
        }
        .map_err(error)?;
        self.buffers.push(buffer);
        let req = unsafe { d.get_buffer_memory_requirements(buffer) };
        let memory = unsafe {
            d.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(c.memory(
                        req.memory_type_bits,
                        vk::MemoryPropertyFlags::HOST_VISIBLE
                            | vk::MemoryPropertyFlags::HOST_COHERENT,
                    )?),
                None,
            )
        }
        .map_err(error)?;
        self.memory.push(memory);
        unsafe { d.bind_buffer_memory(buffer, memory, 0) }.map_err(error)?;
        let pointer =
            unsafe { d.map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty()) }
                .map_err(error)?;
        for y in 0..shm.height as usize {
            // SAFETY: immutable retained mapping geometry was checked above. Raw copy
            // creates no Rust references to concurrently guest-writable pixels.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    shm.mapping.pointer().add(y * shm.stride as usize),
                    pointer.cast::<u8>().add(y * shm.width as usize * 4),
                    shm.width as usize * 4,
                );
            }
        }
        unsafe {
            d.unmap_memory(memory);
        }
        let image = unsafe {
            d.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(vk::Format::B8G8R8A8_UNORM)
                    .extent(vk::Extent3D {
                        width: shm.width,
                        height: shm.height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
        }
        .map_err(error)?;
        self.images.push(image);
        let req = unsafe { d.get_image_memory_requirements(image) };
        let memory = unsafe {
            d.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(
                        c.memory(req.memory_type_bits, vk::MemoryPropertyFlags::empty())?,
                    ),
                None,
            )
        }
        .map_err(error)?;
        self.memory.push(memory);
        unsafe { d.bind_image_memory(image, memory, 0) }.map_err(error)?;
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let barrier = vk::ImageMemoryBarrier::default()
            .image(image)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .subresource_range(range);
        unsafe {
            d.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
            d.cmd_copy_buffer_to_image(
                cmd,
                buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_extent(vk::Extent3D {
                        width: shm.width,
                        height: shm.height,
                        depth: 1,
                    })],
            );
        }
        let barrier = barrier
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ);
        unsafe {
            d.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
        }
        Ok((
            self.view(image, vk::Format::B8G8R8A8_UNORM)?,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            false,
        ))
    }
}
impl Drop for Frame {
    fn drop(&mut self) {
        unsafe {
            for f in self.framebuffers.drain(..) {
                self.device.destroy_framebuffer(f, None);
            }
            if self.descriptors != vk::DescriptorPool::null() {
                self.device.destroy_descriptor_pool(self.descriptors, None);
            }
            for v in self.views.drain(..) {
                self.device.destroy_image_view(v, None);
            }
            if self.pool != vk::CommandPool::null() {
                self.device.destroy_command_pool(self.pool, None);
            }
            for f in self.fences.drain(..) {
                self.device.destroy_fence(f, None);
            }
            for b in self.buffers.drain(..) {
                self.device.destroy_buffer(b, None);
            }
            for i in self.images.drain(..) {
                self.device.destroy_image(i, None);
            }
            for m in self.memory.drain(..) {
                self.device.free_memory(m, None);
            }
        }
    }
}
pub struct Renderer {
    surface: Surface,
    context: Context,
    chain: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    semaphores: Vec<vk::Semaphore>,
    extent: vk::Extent2D,
    format: vk::Format,
    pass: vk::RenderPass,
    descriptor_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    sampler: vk::Sampler,
    vertex: vk::ShaderModule,
    fragment: vk::ShaderModule,
}
impl Renderer {
    /// # Safety
    /// The caller keeps the original target connection/window/surface alive until
    /// this renderer drops, and cancels a Wayland prepared read before native WSI.
    pub unsafe fn new(
        target: Target,
        scene: &WindowSnapshot,
        size: (u32, u32),
    ) -> io::Result<Self> {
        let (context, surface) = unsafe { Context::open(target, scene) }?;
        let mut r = Self {
            surface,
            context,
            chain: vk::SwapchainKHR::null(),
            images: vec![],
            semaphores: vec![],
            extent: vk::Extent2D::default(),
            format: vk::Format::UNDEFINED,
            pass: vk::RenderPass::null(),
            descriptor_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            sampler: vk::Sampler::null(),
            vertex: vk::ShaderModule::null(),
            fragment: vk::ShaderModule::null(),
        };
        r.initialize(size)?;
        Ok(r)
    }
    pub fn matches(&self, scene: &WindowSnapshot) -> bool {
        scene
            .scene
            .iter()
            .filter_map(|s| {
                if let ImageBuffer::Gpu(g) = &s.buffer {
                    g.vulkan_queue()
                } else {
                    None
                }
            })
            .all(|q| {
                q.device() == self.context.queue.device() && q.raw() == self.context.queue.raw()
            })
    }
    fn initialize(&mut self, size: (u32, u32)) -> io::Result<()> {
        let c = &self.context;
        let d = &c.device;
        if !unsafe {
            self.surface.loader.get_physical_device_surface_support(
                c.physical,
                c.queue.family(),
                self.surface.raw,
            )
        }
        .map_err(error)?
        {
            return Err(error("producer queue cannot present to this host surface"));
        }
        let formats = unsafe {
            self.surface
                .loader
                .get_physical_device_surface_formats(c.physical, self.surface.raw)
        }
        .map_err(error)?;
        let format = formats
            .iter()
            .find(|f| {
                f.format == vk::Format::B8G8R8A8_UNORM || f.format == vk::Format::R8G8B8A8_UNORM
            })
            .or_else(|| formats.first())
            .ok_or_else(|| error("no surface format"))?;
        self.format = format.format;
        let mut code = std::io::Cursor::new(include_bytes!("../shaders/desktop.vert.spv"));
        let words = ash::util::read_spv(&mut code).map_err(error)?;
        self.vertex = unsafe {
            d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
        }
        .map_err(error)?;
        let mut code = std::io::Cursor::new(include_bytes!("../shaders/desktop.frag.spv"));
        let words = ash::util::read_spv(&mut code).map_err(error)?;
        self.fragment = unsafe {
            d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
        }
        .map_err(error)?;
        let binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        self.descriptor_layout = unsafe {
            d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding),
                None,
            )
        }
        .map_err(error)?;
        let sets = [self.descriptor_layout];
        let push = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .size(48)];
        self.layout = unsafe {
            d.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&sets)
                    .push_constant_ranges(&push),
                None,
            )
        }
        .map_err(error)?;
        let attachments = [vk::AttachmentDescription::default()
            .format(self.format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];
        let colors = [vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];
        let subpasses = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&colors)];
        let dependencies = [vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)];
        self.pass = unsafe {
            d.create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&attachments)
                    .subpasses(&subpasses)
                    .dependencies(&dependencies),
                None,
            )
        }
        .map_err(error)?;
        self.sampler = unsafe {
            d.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::NEAREST)
                    .min_filter(vk::Filter::NEAREST)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .max_lod(0.),
                None,
            )
        }
        .map_err(error)?;
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(self.vertex)
                .name(c"main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(self.fragment)
                .name(c"main"),
        ];
        let vertex = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_STRIP);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .line_width(1.);
        let samples = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let attachment = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachment);
        let dynamic = vk::PipelineDynamicStateCreateInfo::default()
            .dynamic_states(&[vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR]);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&samples)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(self.layout)
            .render_pass(self.pass);
        self.pipeline = match unsafe {
            d.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None)
        } {
            Ok(v) => v[0],
            Err((v, e)) => {
                for p in v {
                    unsafe {
                        d.destroy_pipeline(p, None);
                    }
                }
                return Err(error(e));
            }
        };
        self.resize(size)
    }
    fn resize(&mut self, size: (u32, u32)) -> io::Result<()> {
        let c = &self.context;
        let d = &c.device;
        let caps = unsafe {
            self.surface
                .loader
                .get_physical_device_surface_capabilities(c.physical, self.surface.raw)
        }
        .map_err(error)?;
        let extent = if caps.current_extent.width != u32::MAX {
            caps.current_extent
        } else {
            vk::Extent2D {
                width: size
                    .0
                    .clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                height: size
                    .1
                    .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
            }
        };
        if extent.width == 0 || extent.height == 0 {
            return Err(error("host surface has zero extent"));
        }
        if self.chain != vk::SwapchainKHR::null() && extent == self.extent {
            return Ok(());
        }
        let loader = ash::khr::swapchain::Device::new(&c.instance, d);
        if self.chain != vk::SwapchainKHR::null() {
            c.queue
                .synchronized(|| unsafe { d.queue_wait_idle(vk::Queue::from_raw(c.queue.raw())) })
                .map_err(error)?;
            for s in self.semaphores.drain(..) {
                unsafe {
                    d.destroy_semaphore(s, None);
                }
            }
            unsafe {
                loader.destroy_swapchain(self.chain, None);
            }
            self.chain = vk::SwapchainKHR::null();
        }
        let formats = unsafe {
            self.surface
                .loader
                .get_physical_device_surface_formats(c.physical, self.surface.raw)
        }
        .map_err(error)?;
        let space = formats
            .iter()
            .find(|f| f.format == self.format)
            .ok_or_else(|| error("surface format changed"))?
            .color_space;
        let alpha = if caps
            .supported_composite_alpha
            .contains(vk::CompositeAlphaFlagsKHR::OPAQUE)
        {
            vk::CompositeAlphaFlagsKHR::OPAQUE
        } else {
            vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED
        };
        let count = caps.min_image_count.max(2);
        let count = if caps.max_image_count == 0 {
            count
        } else {
            count.min(caps.max_image_count)
        };
        self.chain = unsafe {
            loader.create_swapchain(
                &vk::SwapchainCreateInfoKHR::default()
                    .surface(self.surface.raw)
                    .min_image_count(count)
                    .image_format(self.format)
                    .image_color_space(space)
                    .image_extent(extent)
                    .image_array_layers(1)
                    .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                    .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .pre_transform(caps.current_transform)
                    .composite_alpha(alpha)
                    .present_mode(vk::PresentModeKHR::FIFO)
                    .clipped(true),
                None,
            )
        }
        .map_err(error)?;
        self.extent = extent;
        self.images = unsafe { loader.get_swapchain_images(self.chain) }.map_err(error)?;
        for _ in &self.images {
            self.semaphores.push(
                unsafe { d.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }
                    .map_err(error)?,
            );
        }
        Ok(())
    }
    pub fn present(
        &mut self,
        scene: &WindowSnapshot,
        size: (u32, u32),
        bounds: [i32; 4],
    ) -> io::Result<()> {
        self.resize(size)?;
        let c = &self.context;
        let d = &c.device;
        let loader = ash::khr::swapchain::Device::new(&c.instance, d);
        let mut f = Frame::new(d);
        let acquired = f.fence()?;
        let (index, _) = unsafe {
            loader.acquire_next_image(self.chain, u64::MAX, vk::Semaphore::null(), acquired)
        }
        .map_err(error)?;
        unsafe { d.wait_for_fences(&[acquired], true, u64::MAX) }.map_err(error)?;
        f.pool = unsafe {
            d.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(c.queue.family()),
                None,
            )
        }
        .map_err(error)?;
        let cmd = unsafe {
            d.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(f.pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
        }
        .map_err(error)?[0];
        unsafe {
            d.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
        }
        .map_err(error)?;
        let mut sources = vec![];
        for plane in &scene.scene {
            sources.push(f.texture(c, cmd, plane)?);
        }
        let target = f.view(self.images[index as usize], self.format)?;
        let attachments = [target];
        let framebuffer = unsafe {
            d.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(self.pass)
                    .attachments(&attachments)
                    .width(self.extent.width)
                    .height(self.extent.height)
                    .layers(1),
                None,
            )
        }
        .map_err(error)?;
        f.framebuffers.push(framebuffer);
        let pool_sizes = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: sources.len().max(1) as u32,
        }];
        f.descriptors = unsafe {
            d.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .pool_sizes(&pool_sizes)
                    .max_sets(sources.len().max(1) as u32),
                None,
            )
        }
        .map_err(error)?;
        let layouts = vec![self.descriptor_layout; sources.len()];
        let descriptors = unsafe {
            d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(f.descriptors)
                    .set_layouts(&layouts),
            )
        }
        .map_err(error)?;
        let clear = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0., 0., 0., 1.],
            },
        }];
        unsafe {
            d.cmd_begin_render_pass(
                cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.pass)
                    .framebuffer(framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent: self.extent,
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
            d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            d.cmd_set_viewport(
                cmd,
                0,
                &[vk::Viewport {
                    x: 0.,
                    y: 0.,
                    width: self.extent.width as f32,
                    height: self.extent.height as f32,
                    min_depth: 0.,
                    max_depth: 1.,
                }],
            );
            d.cmd_set_scissor(
                cmd,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent: self.extent,
                }],
            );
        }
        for ((plane, (view, layout, srgb)), set) in scene.scene.iter().zip(sources).zip(descriptors)
        {
            let image = [vk::DescriptorImageInfo {
                sampler: self.sampler,
                image_view: view,
                image_layout: layout,
            }];
            unsafe {
                d.update_descriptor_sets(
                    &[vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&image)],
                    &[],
                );
                d.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.layout,
                    0,
                    &[set],
                    &[],
                );
            }
            let rect = [
                (plane.x - bounds[0]) as f32 / bounds[2] as f32 * 2. - 1.,
                (plane.y - bounds[1]) as f32 / bounds[3] as f32 * 2. - 1.,
                plane.destination.0 as f32 / bounds[2] as f32 * 2.,
                plane.destination.1 as f32 / bounds[3] as f32 * 2.,
            ];
            let mut values = [0f32; 12];
            values[..4].copy_from_slice(&rect);
            values[4..8].copy_from_slice(&plane.source);
            values[8] = plane.transform as f32;
            values[9] = f32::from(!plane.buffer.has_alpha());
            values[10] = f32::from(
                [vk::Format::B8G8R8A8_SRGB, vk::Format::R8G8B8A8_SRGB].contains(&self.format),
            );
            values[11] = f32::from(srgb);
            let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            unsafe {
                d.cmd_push_constants(
                    cmd,
                    self.layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    &bytes,
                );
                d.cmd_draw(cmd, 4, 1, 0, 0);
            }
        }
        unsafe {
            d.cmd_end_render_pass(cmd);
            d.end_command_buffer(cmd)
        }
        .map_err(error)?;
        let completed = f.fence()?;
        let commands = [cmd];
        let signals = [self.semaphores[index as usize]];
        let submit = vk::SubmitInfo::default()
            .command_buffers(&commands)
            .signal_semaphores(&signals);
        c.queue
            .synchronized(|| unsafe {
                d.queue_submit(vk::Queue::from_raw(c.queue.raw()), &[submit], completed)
            })
            .map_err(error)?;
        // This is the embedding's own graphics wait, not a guest execution slot or
        // device-wide idle. Source leases stay borrowed until these reads complete.
        unsafe { d.wait_for_fences(&[completed], true, u64::MAX) }.map_err(error)?;
        let chains = [self.chain];
        let indices = [index];
        let info = vk::PresentInfoKHR::default()
            .swapchains(&chains)
            .image_indices(&indices)
            .wait_semaphores(&signals);
        c.queue
            .synchronized(|| unsafe {
                loader.queue_present(vk::Queue::from_raw(c.queue.raw()), &info)
            })
            .map_err(error)?;
        Ok(())
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        let c = &self.context;
        let d = &c.device;
        let _ = c
            .queue
            .synchronized(|| unsafe { d.queue_wait_idle(vk::Queue::from_raw(c.queue.raw())) });
        unsafe {
            for s in self.semaphores.drain(..) {
                d.destroy_semaphore(s, None);
            }
            if self.chain != vk::SwapchainKHR::null() {
                ash::khr::swapchain::Device::new(&c.instance, d)
                    .destroy_swapchain(self.chain, None);
            }
            d.destroy_pipeline(self.pipeline, None);
            d.destroy_sampler(self.sampler, None);
            d.destroy_pipeline_layout(self.layout, None);
            d.destroy_descriptor_set_layout(self.descriptor_layout, None);
            d.destroy_render_pass(self.pass, None);
            d.destroy_shader_module(self.vertex, None);
            d.destroy_shader_module(self.fragment, None);
        }
    }
}
