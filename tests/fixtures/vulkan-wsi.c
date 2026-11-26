// SPDX-License-Identifier: GPL-3.0-or-later
// Same original Vulkan rendering workflow on native XCB and the guest display.
#define VK_USE_PLATFORM_XCB_KHR
#define VK_USE_PLATFORM_WAYLAND_KHR
#include <vulkan/vulkan.h>
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <xcb/xcb.h>
#define OK(call) do { VkResult r=(call); if(r!=VK_SUCCESS){fprintf(stderr,"%s: %d\n",#call,r);exit(1);} } while(0)
#ifdef TRAMES_WSI_WAYLAND
void* test_target_new(void);void* test_target_display(void*);void* test_target_surface(void*);void test_target_free(void*);
#endif
static int headless(void) {
    VkInstanceCreateInfo ii={.sType=VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};VkInstance instance,retained_instance;OK(vkCreateInstance(&ii,NULL,&retained_instance));OK(vkCreateInstance(&ii,NULL,&instance));
    uint32_t count=1;VkPhysicalDevice physical;OK(vkEnumeratePhysicalDevices(instance,&count,&physical));
    VkQueueFamilyProperties queues[64];count=64;vkGetPhysicalDeviceQueueFamilyProperties(physical,&count,queues);uint32_t family=UINT32_MAX;for(uint32_t i=0;i<count;i++)if(queues[i].queueFlags&VK_QUEUE_COMPUTE_BIT){family=i;break;}assert(family!=UINT32_MAX);
    float priority=1;VkDeviceQueueCreateInfo qi={.sType=VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,.queueFamilyIndex=family,.queueCount=1,.pQueuePriorities=&priority};VkDeviceCreateInfo di={.sType=VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,.queueCreateInfoCount=1,.pQueueCreateInfos=&qi};VkDevice device,retained_device;OK(vkCreateDevice(physical,&di,NULL,&retained_device));OK(vkCreateDevice(physical,&di,NULL,&device));VkQueue queue;vkGetDeviceQueue(device,family,0,&queue);
    VkBufferCreateInfo bi={.sType=VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,.size=4096,.usage=VK_BUFFER_USAGE_TRANSFER_DST_BIT};VkBuffer buffer;OK(vkCreateBuffer(device,&bi,NULL,&buffer));VkMemoryRequirements requirement;vkGetBufferMemoryRequirements(device,buffer,&requirement);
    VkPhysicalDeviceMemoryProperties properties;vkGetPhysicalDeviceMemoryProperties(physical,&properties);uint32_t type=UINT32_MAX;for(uint32_t i=0;i<properties.memoryTypeCount;i++)if((requirement.memoryTypeBits&(1u<<i))&&(properties.memoryTypes[i].propertyFlags&6)==6){type=i;break;}assert(type!=UINT32_MAX);
    VkMemoryAllocateInfo mi={.sType=VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,.allocationSize=requirement.size,.memoryTypeIndex=type};VkDeviceMemory memory;OK(vkAllocateMemory(device,&mi,NULL,&memory));OK(vkBindBufferMemory(device,buffer,memory,0));
    VkCommandPoolCreateInfo pi={.sType=VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,.queueFamilyIndex=family};VkCommandPool pool;OK(vkCreateCommandPool(device,&pi,NULL,&pool));VkCommandBufferAllocateInfo ai={.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,.commandPool=pool,.level=VK_COMMAND_BUFFER_LEVEL_PRIMARY,.commandBufferCount=1};VkCommandBuffer command;OK(vkAllocateCommandBuffers(device,&ai,&command));
    VkCommandBufferBeginInfo begin={.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};OK(vkBeginCommandBuffer(command,&begin));vkCmdFillBuffer(command,buffer,0,4096,0x13579bdf);
    VkMemoryBarrier barrier={.sType=VK_STRUCTURE_TYPE_MEMORY_BARRIER,.srcAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT,.dstAccessMask=VK_ACCESS_HOST_READ_BIT};vkCmdPipelineBarrier(command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_HOST_BIT,0,1,&barrier,0,NULL,0,NULL);OK(vkEndCommandBuffer(command));
    VkFenceCreateInfo fi={.sType=VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};VkFence fence;OK(vkCreateFence(device,&fi,NULL,&fence));VkSubmitInfo submit={.sType=VK_STRUCTURE_TYPE_SUBMIT_INFO,.commandBufferCount=1,.pCommandBuffers=&command};OK(vkQueueSubmit(queue,1,&submit,fence));OK(vkWaitForFences(device,1,&fence,VK_TRUE,10000000000ULL));
    uint32_t* values;OK(vkMapMemory(device,memory,0,4096,0,(void**)&values));for(unsigned i=0;i<1024;i++)assert(values[i]==0x13579bdf);vkUnmapMemory(device,memory);
    vkDestroyFence(device,fence,NULL);vkDestroyCommandPool(device,pool,NULL);vkDestroyBuffer(device,buffer,NULL);vkFreeMemory(device,memory,NULL);vkDestroyDevice(device,NULL);vkDestroyDevice(retained_device,NULL);vkDestroyInstance(instance,NULL);vkDestroyInstance(retained_instance,NULL);puts("headless: ok");return 0;
}
int main(int argc,char** argv) {
    setbuf(stdout,NULL);
    if(argc==2 && !strcmp(argv[1],"headless"))return headless();
#if defined(TRAMES_WSI_WAYLAND)
    const char* extensions[]={VK_KHR_SURFACE_EXTENSION_NAME,VK_KHR_WAYLAND_SURFACE_EXTENSION_NAME};
#elif defined(TRAMES_WSI_GUEST)
    const char* extensions[]={VK_KHR_SURFACE_EXTENSION_NAME,VK_KHR_DISPLAY_EXTENSION_NAME};
#else
    const char* extensions[]={VK_KHR_SURFACE_EXTENSION_NAME,VK_KHR_XCB_SURFACE_EXTENSION_NAME};
#endif
    VkApplicationInfo app={.sType=VK_STRUCTURE_TYPE_APPLICATION_INFO,.apiVersion=VK_API_VERSION_1_0};
    VkInstanceCreateInfo ii={.sType=VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,.pApplicationInfo=&app,.enabledExtensionCount=2,.ppEnabledExtensionNames=extensions};
    VkInstance instance;OK(vkCreateInstance(&ii,NULL,&instance));
    uint32_t count=1;VkPhysicalDevice physical;OK(vkEnumeratePhysicalDevices(instance,&count,&physical));assert(count==1);
    VkSurfaceKHR surface;
#if defined(TRAMES_WSI_WAYLAND)
    void* target=test_target_new();VkWaylandSurfaceCreateInfoKHR si={.sType=VK_STRUCTURE_TYPE_WAYLAND_SURFACE_CREATE_INFO_KHR,.display=test_target_display(target),.surface=test_target_surface(target)};OK(vkCreateWaylandSurfaceKHR(instance,&si,NULL,&surface));
#elif defined(TRAMES_WSI_GUEST)
    VkDisplayPropertiesKHR display;count=1;OK(vkGetPhysicalDeviceDisplayPropertiesKHR(physical,&count,&display));assert(count==1);
    VkDisplayModePropertiesKHR mode;count=1;OK(vkGetDisplayModePropertiesKHR(physical,display.display,&count,&mode));assert(count==1);
    VkDisplaySurfaceCreateInfoKHR si={.sType=VK_STRUCTURE_TYPE_DISPLAY_SURFACE_CREATE_INFO_KHR,.displayMode=mode.displayMode,.planeStackIndex=0,.transform=VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR,.globalAlpha=1,.alphaMode=VK_DISPLAY_PLANE_ALPHA_OPAQUE_BIT_KHR,.imageExtent=display.physicalResolution};
    OK(vkCreateDisplayPlaneSurfaceKHR(instance,&si,NULL,&surface));
#else
    int screen_number; xcb_connection_t* connection=xcb_connect(NULL,&screen_number);assert(!xcb_connection_has_error(connection));
    xcb_screen_iterator_t screens=xcb_setup_roots_iterator(xcb_get_setup(connection));for(int i=0;i<screen_number;i++)xcb_screen_next(&screens);
    xcb_screen_t* screen=screens.data;assert(screen);
    xcb_window_t window=xcb_generate_id(connection);
    xcb_create_window(connection,XCB_COPY_FROM_PARENT,window,screen->root,0,0,320,240,0,XCB_WINDOW_CLASS_INPUT_OUTPUT,screen->root_visual,0,NULL);
    const char title[]="Trames WSI native";xcb_change_property(connection,XCB_PROP_MODE_REPLACE,window,XCB_ATOM_WM_NAME,XCB_ATOM_STRING,8,sizeof(title)-1,title);
    xcb_map_window(connection,window);xcb_flush(connection);
    VkXcbSurfaceCreateInfoKHR si={.sType=VK_STRUCTURE_TYPE_XCB_SURFACE_CREATE_INFO_KHR,.connection=connection,.window=window};
    OK(vkCreateXcbSurfaceKHR(instance,&si,NULL,&surface));
#endif
    VkQueueFamilyProperties properties[64];count=64;vkGetPhysicalDeviceQueueFamilyProperties(physical,&count,properties);assert(count<=64);
    uint32_t family=UINT32_MAX;for(uint32_t i=0;i<count;i++){VkBool32 supported=0;OK(vkGetPhysicalDeviceSurfaceSupportKHR(physical,i,surface,&supported));if(supported&&(properties[i].queueFlags&VK_QUEUE_GRAPHICS_BIT)){family=i;break;}}assert(family!=UINT32_MAX);
    float priority=1;VkDeviceQueueCreateInfo qi={.sType=VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,.queueFamilyIndex=family,.queueCount=1,.pQueuePriorities=&priority};
    const char* swapchain_extension=VK_KHR_SWAPCHAIN_EXTENSION_NAME;VkDeviceCreateInfo di={.sType=VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,.queueCreateInfoCount=1,.pQueueCreateInfos=&qi,.enabledExtensionCount=1,.ppEnabledExtensionNames=&swapchain_extension};
    VkDevice device;OK(vkCreateDevice(physical,&di,NULL,&device));VkQueue queue;vkGetDeviceQueue(device,family,0,&queue);
    VkSurfaceCapabilitiesKHR caps;OK(vkGetPhysicalDeviceSurfaceCapabilitiesKHR(physical,surface,&caps));if(caps.currentExtent.width==UINT32_MAX)caps.currentExtent=(VkExtent2D){320,240};assert(caps.currentExtent.width>0);assert(caps.supportedUsageFlags&VK_IMAGE_USAGE_TRANSFER_DST_BIT);
    VkCommandPoolCreateInfo pi={.sType=VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,.flags=VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,.queueFamilyIndex=family};VkCommandPool pool;OK(vkCreateCommandPool(device,&pi,NULL,&pool));
    VkCommandBufferAllocateInfo ai={.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,.commandPool=pool,.level=VK_COMMAND_BUFFER_LEVEL_PRIMARY,.commandBufferCount=1};VkCommandBuffer command;OK(vkAllocateCommandBuffers(device,&ai,&command));
    VkFenceCreateInfo fi={.sType=VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};VkFence acquire,finished;OK(vkCreateFence(device,&fi,NULL,&acquire));OK(vkCreateFence(device,&fi,NULL,&finished));
    VkSwapchainKHR old=VK_NULL_HANDLE;unsigned frames=0;
    for(unsigned round=0;round<2;round++) {
        VkSwapchainCreateInfoKHR ci={.sType=VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR,.surface=surface,.minImageCount=caps.minImageCount,.imageFormat=VK_FORMAT_B8G8R8A8_UNORM,.imageColorSpace=VK_COLOR_SPACE_SRGB_NONLINEAR_KHR,.imageExtent=caps.currentExtent,.imageArrayLayers=1,.imageUsage=VK_IMAGE_USAGE_TRANSFER_DST_BIT|VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT,.imageSharingMode=VK_SHARING_MODE_EXCLUSIVE,.preTransform=VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR,.compositeAlpha=VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR,.presentMode=VK_PRESENT_MODE_FIFO_KHR,.clipped=VK_TRUE,.oldSwapchain=old};
        VkSwapchainKHR chain;OK(vkCreateSwapchainKHR(device,&ci,NULL,&chain));if(old)vkDestroySwapchainKHR(device,old,NULL);
        count=0;OK(vkGetSwapchainImagesKHR(device,chain,&count,NULL));assert(count>0 && count<=4);VkImage images[4];OK(vkGetSwapchainImagesKHR(device,chain,&count,images));
        for(unsigned frame=0;frame<8;frame++) {
            uint32_t index;OK(vkAcquireNextImageKHR(device,chain,10000000000ULL,VK_NULL_HANDLE,acquire,&index));OK(vkWaitForFences(device,1,&acquire,VK_TRUE,10000000000ULL));OK(vkResetFences(device,1,&acquire));
            OK(vkResetCommandBuffer(command,0));VkCommandBufferBeginInfo begin={.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};OK(vkBeginCommandBuffer(command,&begin));
            VkImageMemoryBarrier barrier={.sType=VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,.oldLayout=VK_IMAGE_LAYOUT_UNDEFINED,.newLayout=VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,.srcQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED,.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED,.image=images[index],.dstAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT,.subresourceRange={VK_IMAGE_ASPECT_COLOR_BIT,0,1,0,1}};
            vkCmdPipelineBarrier(command,VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,NULL,0,NULL,1,&barrier);
            VkClearColorValue color={.float32={round?0:1,round?1:0,0,1}};vkCmdClearColorImage(command,images[index],VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,&color,1,&barrier.subresourceRange);
            barrier.oldLayout=VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL;barrier.newLayout=VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;barrier.srcAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT;barrier.dstAccessMask=0;
            vkCmdPipelineBarrier(command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,0,0,NULL,0,NULL,1,&barrier);OK(vkEndCommandBuffer(command));
            VkSubmitInfo submit={.sType=VK_STRUCTURE_TYPE_SUBMIT_INFO,.commandBufferCount=1,.pCommandBuffers=&command};OK(vkQueueSubmit(queue,1,&submit,finished));OK(vkWaitForFences(device,1,&finished,VK_TRUE,10000000000ULL));OK(vkResetFences(device,1,&finished));
            VkPresentInfoKHR present={.sType=VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,.swapchainCount=1,.pSwapchains=&chain,.pImageIndices=&index};OK(vkQueuePresentKHR(queue,&present));frames++;
        }
        OK(vkDeviceWaitIdle(device));OK(vkResetCommandBuffer(command,0));old=chain;
    }
    printf("wsi-ready frames=%u\n",frames);char ack;assert(read(STDIN_FILENO,&ack,1)==1 && ack=='q');
    vkDestroySwapchainKHR(device,old,NULL);vkDestroyFence(device,acquire,NULL);vkDestroyFence(device,finished,NULL);vkDestroyCommandPool(device,pool,NULL);vkDestroyDevice(device,NULL);vkDestroySurfaceKHR(instance,surface,NULL);vkDestroyInstance(instance,NULL);
#if defined(TRAMES_WSI_WAYLAND)
    test_target_free(target);
#elif !defined(TRAMES_WSI_GUEST)
    xcb_destroy_window(connection,window);xcb_disconnect(connection);
#endif
    puts("wsi: ok");return 0;
}
