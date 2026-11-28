// SPDX-License-Identifier: GPL-3.0-or-later
//! Named environments reuse the shared manifest, private store and host protocols.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::OpenOptionsExt, net::UnixStream, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use trames_distro::{
    environment::{self as model, Manifest, Result},
    registry::{self, Registry},
};
const HELP:&str="Usage: trames [-e NAME_OR_UUID] COMMAND\n  env create NAME [--distro alpine|debian] [--rootfs PATH|--archive FILE] [--config TOML]\n  env ls [--json|-q] | inspect NAME [--json] | path NAME\n  env enable|disable NAME [--now]   Login autostart through a systemd user service\n  env start|stop|restart [NAME] | logs NAME | rename NAME NEW | remove NAME --yes\n  env clone NAME NEW | backup NAME FILE | restore NEW FILE | import-store NEW DIRECTORY\n  env share NAME ALIAS HOST GUEST [ro|rw]\n  env publish NAME HOST:PORT GUEST_PORT [tcp|udp] | unpublish NAME HOST:PORT [tcp|udp]\n  env grant NAME CAPABILITY on|off\n  network ls [--json] | create NAME [--internal] | inspect NAME | remove NAME\n  config show [--json] | path | validate | get KEY | set KEY VALUE [KEY VALUE...] | edit\n  config --global ...\n  shell | exec [-t] [--setenv K=V] [--cwd DIR] -- COMMAND [ARG...]\n  session ls [--json|-q] | attach ID\nExisting environments start on demand; shell/exec never download or create images.\n";
pub fn root() -> Result<Registry> {
    let p = if let Some(p) = std::env::var_os("TRAMES_HOME") {
        PathBuf::from(p)
    } else if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(p).join("trames")
    } else {
        PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?)
            .join(".local/share/trames")
    };
    Registry::open(&p)
}
fn chosen(r: &Registry, n: Option<&str>) -> Result<String> {
    Ok(n.map(str::to_owned)
        .unwrap_or(r.config()?.default_environment))
}
pub fn console_path(dir: &Path) -> Result<PathBuf> {
    let root = dir
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid environment directory")?;
    let i = registry::instance(dir)?;
    Ok(Registry::at(root)
        .socket(&i.uuid)?
        .with_extension("console"))
}
fn cpus(c: &Manifest) -> Result<usize> {
    c.resources.cpus.resolve(
        std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(32),
    )
}
pub(crate) fn validate(c: &Manifest) -> Result<()> {
    c.validate()?;
    cpus(c)?;
    c.components.thunking.resolve(false)?;
    c.components.vaapi.resolve(false)?;
    if c.environment.kind != trames_distro::boot::Target::Linux {
        return Err("managed Android boot is not implemented by the Linux product".into());
    }
    if c.image.distribution == "alpine" && c.boot.mode == model::Init::Systemd {
        return Err("Alpine does not include systemd".into());
    }
    Ok(())
}
fn profile(r: &Registry, n: &str) -> Result<Manifest> {
    let c = registry::load(&r.require(n)?)?;
    validate(&c)?;
    Ok(c)
}
fn grants(dir: &Path, c: &Manifest) -> Result<model::Grant> {
    let p = dir.join("grants.toml");
    if !p.exists() {
        return Ok(model::Grant::default());
    }
    let a: model::Authorization = model::decode(&fs::read_to_string(p)?)?;
    Ok(a.for_environment(&c.environment.uuid)?.clone())
}
fn state(r: &Registry, n: &str) -> Result<serde_json::Value> {
    let dir = r.require(n)?;
    let i = registry::instance(&dir)?;
    let config = profile(r, n);
    let busy = r.running(n)?;
    let running =
        busy && UnixStream::connect(trames_cli_common::exec::path(&console_path(&dir)?)).is_ok();
    Ok(
        serde_json::json!({"name":i.name,"uuid":i.uuid,"distribution":i.distribution,"state":if config.is_err(){"needs attention"}else if running{"running"}else if busy{"busy"}else{"stopped"},"directory":dir,"running":running,"error":config.as_ref().err().map(ToString::to_string),"config":config.ok(),"service":crate::user_service::inspect(r,n).ok(),"last_exit":fs::read(dir.join("exit.json")).ok().and_then(|b|serde_json::from_slice::<serde_json::Value>(&b).ok())}),
    )
}
pub fn dispatch(selected: Option<&str>, op: &str, args: Vec<String>) -> Result<Option<u8>> {
    if op == "completion" {
        let shell = args.first().map(String::as_str).unwrap_or("bash");
        match shell{"bash"=>println!("complete -W 'env network config shell exec session status doctor probe run read-file' trames"),"zsh"=>println!("#compdef trames\n_arguments '1:command:(env network config shell exec session status doctor probe run read-file)'"),"fish"=>println!("complete -c trames -f -a 'env network config shell exec session status doctor probe run read-file'"),_=>return Err("completion requires bash, zsh or fish".into())}
        return Ok(Some(0));
    }
    if matches!(
        op,
        "env" | "network" | "config" | "status" | "shell" | "exec" | "session"
    ) && (args
        .first()
        .is_some_and(|s| matches!(s.as_str(), "--help" | "-h"))
        || op == "env"
            && args
                .get(1)
                .is_some_and(|s| matches!(s.as_str(), "--help" | "-h")))
    {
        print!("{HELP}");
        return Ok(Some(0));
    }
    if op == "network" {
        return trames_distro::networks::Networks::at(&root()?.root)
            .dispatch(&args)
            .map(Some);
    }
    if op == "env" {
        return commands(selected, &args).map(Some);
    }
    if op == "config" {
        return settings(selected, &args).map(Some);
    }
    if op == "status" {
        let r = root()?;
        let n = chosen(&r, selected)?;
        println!("{}", serde_json::to_string_pretty(&state(&r, &n)?)?);
        return Ok(Some(0));
    }
    if !matches!(op, "shell" | "exec" | "run" | "--" | "session") {
        return Ok(None);
    }
    let explicit = op != "--" && explicit_storage(&args);
    if explicit {
        if selected.is_some() {
            return Err("choose -e NAME or explicit --rootfs/--store/--socket".into());
        }
        return Ok(None);
    }
    if op == "run" && selected.is_none() {
        return Ok(None);
    }
    let r = root()?;
    let n = chosen(&r, selected)?;
    if op == "session" {
        let console = console_path(&r.require(&n)?)?;
        match args.first().map(String::as_str).unwrap_or("ls") {
            "ls" | "list" => {
                if !r.running(&n)? {
                    if args.iter().any(|v| v == "--json") {
                        println!("[]");
                    }
                    return Ok(Some(0));
                }
                trames_cli_common::console::list(&console, &args[1..])?;
            }
            "attach" if args.len() == 2 => {
                if !r.running(&n)? {
                    return Err("environment is stopped; use shell to open a new terminal".into());
                }
                trames_cli_common::console::attach(&console, args[1].parse()?)?
            }
            _ => return Err("usage: session ls [--json|-q] | attach ID".into()),
        }
        return Ok(Some(0));
    }
    if op == "shell" {
        if !args.is_empty() {
            return Err(
                "named shell takes no options; configure the environment or use exec -t".into(),
            );
        }
        start(&r, &n)?;
        trames_cli_common::console::attach(&console_path(&r.require(&n)?)?, 0)?;
        return Ok(Some(0));
    }
    let request = trames_cli_common::arguments::parse_exec(&args, op == "--")?;
    start(&r, &n)?;
    Ok(Some(trames_cli_common::exec::run(
        &console_path(&r.require(&n)?)?,
        request,
    )?))
}
fn commands(selected: Option<&str>, args: &[String]) -> Result<u8> {
    let op = args.first().map(String::as_str).unwrap_or("ls");
    if op == "help" {
        print!("{HELP}");
        return Ok(0);
    }
    let r = root()?;
    match op {
        "ls" | "list" => {
            if args.len() > 2
                || args
                    .get(1)
                    .is_some_and(|v| !matches!(v.as_str(), "--json" | "-q"))
            {
                return Err("usage: env ls [--json|-q]".into());
            }
            let rows = r
                .instances()?
                .iter()
                .map(|(_, i)| state(&r, &i.uuid))
                .collect::<Result<Vec<_>>>()?;
            if args.get(1).is_some_and(|v| v == "--json") {
                println!("{}", serde_json::to_string_pretty(&rows)?)
            } else {
                for row in rows {
                    if args.get(1).is_some_and(|v| v == "-q") {
                        println!("{}", row["name"].as_str().unwrap())
                    } else {
                        println!(
                            "{}\t{}\t{}\t{}",
                            row["name"].as_str().unwrap(),
                            row["state"].as_str().unwrap(),
                            row["distribution"].as_str().unwrap(),
                            row["uuid"].as_str().unwrap()
                        )
                    }
                }
            }
        }
        "create" => create(&r, &args[1..])?,
        "enable" | "disable" => crate::user_service::dispatch(&r, op, &args[1..])?,
        "_stop" if args.len() == 2 => stop_direct(&r, &args[1])?,
        "_service" if args.len() == 2 => return serve(&r, &args[1], None, true),
        "start" | "stop" | "restart" => {
            let n = chosen(&r, args.get(1).map(String::as_str).or(selected))?;
            if args.len() > 2 {
                return Err("start/stop/restart accept one environment name".into());
            }
            if op != "start" {
                stop(&r, &n)?;
            }
            if op != "stop" {
                start(&r, &n)?;
            }
        }
        "_serve" => {
            if args.len() != 3 {
                return Err("invalid internal environment worker arguments".into());
            }
            return serve_fd(&r, &args[1], args[2].parse()?);
        }
        "inspect" | "status" => {
            let n = args.get(1).ok_or("environment name required")?;
            println!("{}", serde_json::to_string_pretty(&state(&r, n)?)?);
        }
        "path" => println!(
            "{}",
            r.require(args.get(1).ok_or("environment name required")?)?
                .display()
        ),
        "logs" => {
            let name = args.get(1).ok_or("environment name required")?;
            if crate::user_service::installed(&r, name)? {
                return crate::user_service::logs(&r, name).map(|()| 0);
            }
            use std::io::{Seek, SeekFrom};
            let i =
                registry::instance(&r.require(args.get(1).ok_or("environment name required")?)?)?;
            let p = r.root.join("logs").join(format!("{}.log", i.uuid));
            let mut f = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(p)?;
            f.seek(SeekFrom::Start(f.metadata()?.len().saturating_sub(65536)))?;
            io::copy(&mut f, &mut io::stdout().lock())?;
        }
        "rename" => {
            if args.len() != 3 {
                return Err("usage: env rename NAME NEW".into());
            }
            let _metadata = r.metadata_lock()?;
            let _owner = r.owner_lock(&args[1])?;
            registry::name(&args[2])?;
            if r.directory(&args[2])?.exists() {
                return Err("environment name already exists".into());
            }
            let dir = r.require(&args[1])?;
            let mut c = profile(&r, &args[1])?;
            let old_name = c.environment.name.clone();
            c.environment.name = args[2].clone();
            r.register(&dir, &c, dir.join("env.toml").is_file())?;
            let mut cli = r.config()?;
            if cli.default_environment == old_name || cli.default_environment == c.environment.uuid
            {
                cli.default_environment = args[2].clone();
                registry::atomic_write(
                    &r.root.join("config.toml"),
                    model::encode(&cli)?.as_bytes(),
                )?;
            }
        }
        "remove" => {
            if args.len() != 3 || args[2] != "--yes" {
                return Err("usage: env remove NAME --yes".into());
            }
            let _lifecycle = r.lifecycle_lock(&args[1])?;
            if crate::user_service::installed(&r, &args[1])? {
                return Err("disable the environment user service before removing it".into());
            }
            let _metadata = r.metadata_lock()?;
            let _owner = r.owner_lock(&args[1])?;
            let dir = r.require(&args[1])?;
            let vfs = trames_vfs::Vfs::open_disk(dir.join("store"), Default::default())?;
            let trash = r.stage()?;
            fs::remove_dir(&trash.0)?;
            fs::rename(dir, &trash.0)?;
            drop(vfs);
        }
        "share" => share(&r, &args[1..])?,
        "publish" | "unpublish" => publication(&r, op, &args[1..])?,
        "grant" => grant(&r, &args[1..])?,
        "clone" | "backup" | "export" | "restore" | "import-store" => {
            data_command(&r, op, &args[1..])?
        }
        _ => return Err(format!("unknown environment operation '{op}'\n{HELP}").into()),
    }
    Ok(0)
}
fn create(r: &Registry, args: &[String]) -> Result<()> {
    let positional = args.first().filter(|s| !s.starts_with('-'));
    let mut c = r.config()?.defaults;
    let mut source = None;
    let mut archive = None;
    let mut profile_path = None;
    let mut distro = None;
    let mut sha = None;
    let mut settings = vec![];
    let mut it = args[usize::from(positional.is_some())..].iter();
    while let Some(k) = it.next() {
        let v = it.next().ok_or("missing create option value")?;
        match k.as_str() {
            "--rootfs" => source = Some(PathBuf::from(v)),
            "--archive" => archive = Some(PathBuf::from(v)),
            "--config" => profile_path = Some(PathBuf::from(v)),
            "--distro" => distro = Some(v.as_str()),
            "--sha256" => sha = Some(v.clone()),
            "--cpus" => settings.extend(["resources.cpus".to_string(), v.clone()]),
            "--memory" => settings.extend(["resources.memory".to_string(), v.clone()]),
            "--init" => settings.extend(["boot.mode".to_string(), v.clone()]),
            _ => return Err(format!("unknown create option {k}").into()),
        }
    }
    if let Some(d) = distro {
        c = c.using_image(d);
    }
    if let Some(path) = &profile_path {
        c = Manifest::parse(&fs::read_to_string(path)?)?;
    }
    if let Some(n) = positional {
        c.environment.name = n.clone();
    }
    registry::name(&c.environment.name)?;
    c.environment.uuid = model::uuid()?;
    for pair in settings.as_chunks::<2>().0.iter() {
        registry::set(&mut c, &pair[0], &pair[1])?;
    }
    if source.is_none() {
        source = c.image.source.as_ref().map(PathBuf::from);
    }
    if source.is_some() && archive.is_some() {
        return Err("choose --rootfs or --archive".into());
    }
    if source.is_some() {
        c.image.distribution = "custom".into();
    }
    if sha.is_none() && !c.image.sha256.is_empty() {
        sha = Some(c.image.sha256.clone());
    }
    if source.is_none()
        && sha.as_ref().is_some_and(|s| {
            catalog(&c.image.distribution).is_ok_and(|(_, expected)| s != expected)
        })
    {
        return Err("expected checksum differs from the pinned distribution archive".into());
    }
    validate(&c)?;
    {
        let _guard = r.metadata_lock()?;
        if r.directory(&c.environment.name)?.exists() {
            return Err("environment name already exists".into());
        }
    }
    let stage = r.stage()?;
    let mut vfs = trames_vfs::Vfs::open_disk(stage.0.join("store"), Default::default())?;
    let explicit = profile_path.is_some() || !settings.is_empty();
    let local = if let Some(p) = &source {
        p.clone()
    } else {
        cached(r, &c.image.distribution, archive.as_deref())?
    };
    c.image.sha256 = vfs.populate_staged(|vfs| -> Result<String> {
        if source.is_some() {
            trames_distro::rootfs::install(&local, vfs, sha.as_deref())
        } else {
            let mut f = File::open(&local)?;
            match c.image.distribution.as_str() {
                "alpine" => {
                    trames_distro::install_alpine(&mut f, vfs)?;
                }
                "debian" => {
                    trames_distro::install_debian(&mut f, vfs)?;
                }
                _ => {
                    return Err(
                        "distribution must be alpine or debian; custom images use --rootfs".into(),
                    )
                }
            }
            Ok(catalog(&c.image.distribution)?.1.into())
        }
    })?;
    if !explicit {
        if let Some(mut guest) = model::guest(&mut vfs, &c)? {
            guest.environment = c.environment.clone();
            guest.image = c.image.clone();
            c = guest;
        }
    }
    validate(&c)?;
    trames_distro::install_resolver(&mut vfs, b"nameserver 169.254.0.53\n")?;
    trames_distro::systemd::ensure_machine_identity(&mut vfs)?;
    let command = if c.boot.mode == model::Init::Systemd {
        "/sbin/init".to_string()
    } else if !c.session.shell.is_empty() {
        c.session.shell[0].clone()
    } else {
        c.resolve_login(&mut vfs).shell
    };
    vfs.executable_file_at(trames_vfs::AT_FDCWD, command.as_bytes(), 0)?;
    vfs.sync()?;
    drop(vfs);
    r.register(&stage.0, &c, explicit)?;
    r.publish(&stage.0, &c)?;
    println!("Created {} ({})", c.environment.name, c.image.distribution);
    Ok(())
}
fn catalog(d: &str) -> Result<(&'static str, &'static str)> {
    match d {
        "alpine" => Ok((trames_distro::ALPINE_URL, trames_distro::ALPINE_SHA256)),
        "debian" => Ok((trames_distro::DEBIAN_URL, trames_distro::DEBIAN_SHA256)),
        _ => Err("distribution must be alpine or debian".into()),
    }
}
fn cached(r: &Registry, d: &str, archive: Option<&Path>) -> Result<PathBuf> {
    let (url, sha) = catalog(d)?;
    if let Some(p) = archive {
        if registry::digest(p)? != sha {
            return Err("distribution archive checksum mismatch".into());
        }
        return Ok(p.into());
    }
    let path = r.root.join("cache").join(format!("{d}-{sha}.tar.gz"));
    if path.is_file() && registry::digest(&path)? == sha {
        return Ok(path);
    }
    let temp = path.with_extension(format!("partial-{}", model::uuid()?));
    let result = (|| {
        let status = Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--max-filesize",
                "268435456",
                "--output",
            ])
            .arg(&temp)
            .arg(url)
            .status()?;
        if !status.success() || registry::digest(&temp)? != sha {
            return Err("rootfs download failed or checksum mismatch".into());
        }
        File::open(&temp)?.sync_all()?;
        fs::rename(&temp, &path)?;
        Ok(path)
    })();
    let _ = fs::remove_file(temp);
    result
}
fn settings(selected: Option<&str>, args: &[String]) -> Result<u8> {
    let r = root()?;
    let _metadata = r.metadata_lock()?;
    let global = args.first().is_some_and(|v| v == "--global");
    let args = args[usize::from(global)..]
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let op = args.first().copied().unwrap_or("show");
    let mut cli = r.config()?;
    let dir = if global {
        None
    } else {
        Some(r.require(&chosen(&r, selected)?)?)
    };
    let path = dir
        .as_ref()
        .map(|d| d.join("env.toml"))
        .unwrap_or_else(|| r.root.join("config.toml"));
    if op == "path" {
        if args.len() != 1 {
            return Err("usage: config path".into());
        }
        println!("{}", path.display());
        return Ok(0);
    }
    let mut c = if let Some(d) = &dir {
        registry::load(d)?
    } else {
        cli.defaults.clone()
    };
    match op {
        "show" | "list" if args.len() <= 2 && args.get(1).is_none_or(|v| *v == "--json") => {
            if args.contains(&"--json") {
                let value = if global { serde_json::to_value(&cli)? } else { serde_json::to_value(&c)? };
                println!("{}",serde_json::to_string_pretty(&value)?);
            } else {print!("{}",if global {model::encode(&cli)?} else {model::encode(&c)?});}
        }
        "get" if args.len() == 2 => {
            let value = if global {serde_json::to_value(&cli)?} else {serde_json::to_value(&c)?};
            let mut field = &value;
            for key in args[1].split('.') {field=field.get(key).ok_or("unknown configuration field")?;}
            if let Some(text)=field.as_str() {println!("{text}");} else {println!("{field}");}
        }
        "validate" if args.len() == 1 => {validate(&c)?;println!("Valid environment configuration");}
        "set" if args.len() >= 3 && args.len()%2 == 1 => {
            for pair in args[1..].as_chunks::<2>().0 {
                if global && pair[0] == "default_environment" {r.require(pair[1])?;cli.default_environment=pair[1].into();}
                else if global {registry::set_default(&mut c,pair[0].strip_prefix("defaults.").ok_or("global keys use defaults.TABLE.FIELD")?,pair[1])?;}
                else {registry::set(&mut c,pair[0],pair[1])?;}
            }
            validate(&c)?;
            if global {cli.defaults=c;registry::atomic_write(&path,model::encode(&cli)?.as_bytes())?;}
            else {r.save(dir.as_ref().unwrap(),&c)?;}
        }
        "edit" if args.len() == 1 => {
            let temp=r.root.join(format!(".edit-{}.toml",model::uuid()?));
            let result: Result<()> = (|| {
                let initial=if global {model::encode(&cli)?} else {model::encode(&c)?};
                registry::atomic_write(&temp,initial.as_bytes())?;
                let editor=std::env::var_os("EDITOR").unwrap_or_else(||"vi".into());
                if !Command::new(editor).arg(&temp).status()?.success() {return Err("editor failed".into());}
                let text=fs::read_to_string(&temp)?;
                if global {
                    let next: registry::CliConfig=model::decode(&text)?;
                    if next.version!=1 {return Err("unsupported CLI config version".into());}
                    registry::name(&next.default_environment)?;validate(&next.defaults)?;
                    registry::atomic_write(&path,model::encode(&next)?.as_bytes())?;
                } else {let next=Manifest::parse(&text)?;validate(&next)?;r.save(dir.as_ref().unwrap(),&next)?;}
                Ok(())
            })();
            let _=fs::remove_file(temp);result?;
        }
        _ => return Err("usage: config show [--json] | path | validate | get KEY | set KEY VALUE [KEY VALUE...] | edit".into()),
    }
    Ok(0)
}
fn host_address(value: &str) -> Result<std::net::SocketAddr> {
    let address = if let Ok(port) = value.parse::<u16>() {
        std::net::SocketAddr::from(([127, 0, 0, 1], port))
    } else {
        value.parse()?
    };
    if address.port() == 0 {
        return Err("published host port must be nonzero".into());
    }
    Ok(address)
}
fn publication(r: &Registry, op: &str, args: &[String]) -> Result<()> {
    let expected = if op == "publish" { 3..=4 } else { 2..=3 };
    if !expected.contains(&args.len()) {
        return Err("usage: env publish NAME HOST:PORT GUEST_PORT [tcp|udp] | unpublish NAME HOST:PORT [tcp|udp]".into());
    }
    let _metadata = r.metadata_lock()?;
    let dir = r.require(&args[0])?;
    let mut c = profile(r, &args[0])?;
    let host = host_address(&args[1])?;
    let protocol = args
        .get(if op == "publish" { 3 } else { 2 })
        .map(String::as_str)
        .unwrap_or("tcp");
    if !["tcp", "udp"].contains(&protocol) {
        return Err("protocol must be tcp or udp".into());
    }
    c.network
        .publish
        .retain(|p| p.host != host || p.protocol != protocol);
    if op == "publish" {
        if !grants(&dir, &c)?.effective(&c.interop).ports.enabled {
            return Err("ports request or host grant is disabled".into());
        }
        c.network.publish.push(model::Publish {
            protocol: protocol.into(),
            host,
            guest: args[2].parse()?,
        });
    }
    validate(&c)?;
    r.save(&dir, &c)
}
fn grant(r: &Registry, args: &[String]) -> Result<()> {
    if args.len() != 3 {
        return Err("usage: env grant NAME CAPABILITY on|off".into());
    }
    let _metadata = r.metadata_lock()?;
    let dir = r.require(&args[0])?;
    let c = profile(r, &args[0])?;
    let mut g = grants(&dir, &c)?;
    g.set_permission(
        &args[1],
        match args[2].as_str() {
            "on" => true,
            "off" => false,
            _ => return Err("grant requires on or off".into()),
        },
    )?;
    registry::atomic_write(
        &dir.join("grants.toml"),
        model::encode(&model::Authorization {
            environment_uuid: c.environment.uuid,
            grant: g,
        })?
        .as_bytes(),
    )
}
fn share(r: &Registry, args: &[String]) -> Result<()> {
    if !(4..=5).contains(&args.len()) {
        return Err("usage: env share NAME ALIAS HOST GUEST [ro|rw]".into());
    }
    registry::name(&args[1])?;
    let _metadata = r.metadata_lock()?;
    let dir = r.require(&args[0])?;
    let mut c = profile(r, &args[0])?;
    let mut g = grants(&dir, &c)?;
    let host = fs::canonicalize(&args[2])?;
    if !host.is_dir() {
        return Err("share must be a directory".into());
    }
    let readonly = match args.get(4).map(String::as_str).unwrap_or("rw") {
        "ro" => true,
        "rw" => false,
        _ => return Err("expected ro or rw".into()),
    };
    g.shares.insert(
        args[1].clone(),
        model::Share {
            host: host.to_str().ok_or("share path must be UTF-8")?.into(),
            readonly,
        },
    );
    c.mounts.retain(|m| m.target != args[3]);
    c.mounts.push(model::Mount {
        target: args[3].clone(),
        share: Some(args[1].clone()),
        host: None,
        readonly,
    });
    validate(&c)?;
    registry::atomic_write(
        &dir.join("grants.toml"),
        model::encode(&model::Authorization {
            environment_uuid: c.environment.uuid.clone(),
            grant: g,
        })?
        .as_bytes(),
    )?;
    r.save(&dir, &c)
}
fn start(r: &Registry, n: &str) -> Result<()> {
    // Reuse a ready owner, including an ordinary worker enabled for the next login.
    // Steady-state exec does not spawn systemctl or reset an automatic restart budget.
    if r.running(n)?
        && UnixStream::connect(trames_cli_common::exec::path(&console_path(
            &r.require(n)?,
        )?))
        .is_ok()
    {
        return Ok(());
    }
    if crate::user_service::installed(r, n)? {
        return crate::user_service::control(r, n, "start");
    }
    let _lifecycle = r.lifecycle_lock(n)?;
    let dir = r.require(n)?;
    let c = profile(r, n)?;
    if r.running(n)? {
        if UnixStream::connect(trames_cli_common::exec::path(&console_path(&dir)?)).is_ok() {
            return Ok(());
        }
        return Err("environment has a live owner but its command socket is unavailable".into());
    }
    let i = registry::instance(&dir)?;
    let logpath = r.root.join("logs").join(format!("{}.log", i.uuid));
    if fs::metadata(&logpath).is_ok_and(|m| m.len() > 8 * 1024 * 1024) {
        let previous = logpath.with_extension("log.1");
        let _ = fs::remove_file(&previous);
        fs::rename(&logpath, previous)?;
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&logpath)?;
    let (mut ready, worker) = UnixStream::pair()?;
    ready.set_read_timeout(Some(Duration::from_secs(60)))?;
    let fd = worker.as_raw_fd();
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["env", "_serve", &c.environment.uuid, &fd.to_string()])
        .env("TRAMES_HOME", &r.root)
        .env_remove("NOTIFY_SOCKET")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // SAFETY: child-only async-signal-safe calls on its inherited readiness FD.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 || libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    drop(worker);
    let mut b = [0];
    match ready.read_exact(&mut b) {
        Ok(()) if b[0] == b'R' => Ok(()),
        Ok(()) if b[0] == b'E' => {
            let mut size = [0; 4];
            ready.read_exact(&mut size)?;
            let len = u32::from_be_bytes(size) as usize;
            if len > 8192 {
                return Err("invalid worker startup response".into());
            }
            let mut bytes = vec![0; len];
            ready.read_exact(&mut bytes)?;
            let _ = child.wait();
            Err(String::from_utf8_lossy(&bytes).into_owned().into())
        }
        result => {
            drop(ready);
            if child.try_wait()?.is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
            Err(format!(
                "environment startup failed ({result:?}); inspect {}",
                logpath.display()
            )
            .into())
        }
    }
}
fn stop(r: &Registry, n: &str) -> Result<()> {
    if crate::user_service::installed(r, n)? {
        crate::user_service::control(r, n, "stop")?;
    }
    stop_direct(r, n)
}
pub(crate) fn stop_direct(r: &Registry, n: &str) -> Result<()> {
    let _lifecycle = r.lifecycle_lock(n)?;
    if !r.running(n)? {
        return Ok(());
    }
    let mut socket = UnixStream::connect(r.socket(n)?)?;
    socket.set_read_timeout(Some(Duration::from_secs(60)))?;
    socket.write_all(b"S")?;
    let mut b = [0];
    match socket.read_exact(&mut b) {
        Ok(()) if b[0] == b'D' => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof && !r.running(n)? => Ok(()),
        _ => Err("environment stop failed; inspect env logs".into()),
    }
}
fn serve_fd(r: &Registry, n: &str, fd: i32) -> Result<u8> {
    if fd < 3 {
        return Err("invalid readiness descriptor".into());
    }
    let mut kind: libc::c_int = 0;
    let mut len = std::mem::size_of_val(&kind) as libc::socklen_t;
    // SAFETY: cold validation of a caller-supplied descriptor before taking ownership.
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut len,
        )
    } != 0
        || kind != libc::SOCK_STREAM
    {
        return Err("readiness descriptor must be an open Unix stream".into());
    }
    // SAFETY: this internal worker exclusively owns the validated descriptor passed by start.
    serve(r, n, Some(unsafe { UnixStream::from_raw_fd(fd) }), false)
}
fn serve(r: &Registry, n: &str, mut ready: Option<UnixStream>, systemd: bool) -> Result<u8> {
    let mut service = None;
    let result = (|| {
        let mut notifier = if systemd {
            Some(crate::user_service::Notifier::open()?)
        } else {
            None
        };
        let _owner = r.owner_lock(n)?;
        let dir = r.require(n)?;
        trames_kvm::Doctor::inspect().require_probe()?;
        let mut vfs = trames_vfs::Vfs::open_disk(dir.join("store"), Default::default())?;
        let _ = fs::remove_file(dir.join("exit.json"));
        let c = registry::effective(&dir, &r.config()?.defaults, &mut vfs)?;
        validate(&c)?;
        drop(vfs);
        let mut args = vec![
            "--cpus".into(),
            cpus(&c)?.to_string(),
            "--memory-mib".into(),
            c.resources
                .memory_limit
                .as_ref()
                .unwrap_or(&c.resources.memory)
                .clone(),
            "--network".into(),
            match c.network.mode {
                model::NetworkMode::None => "off",
                model::NetworkMode::Host => "host",
                model::NetworkMode::Bridge => "outbound",
            }
            .into(),
        ];
        if c.components.vulkan.resolve(true)? {
            args.push("--vulkan".into());
        }
        if c.components.audio {
            args.push("--audio".into());
        }
        if c.components.audio_input {
            args.push("--audio-input".into());
        }
        for (k, v) in &c.kernel.sysctl {
            args.extend(["--sysctl".into(), format!("{k}={v}")]);
        }
        let g = grants(&dir, &c)?;
        if c.network.mode == model::NetworkMode::Bridge {
            args.extend(trames_distro::networks::Networks::at(&r.root).arguments(&c, &g)?);
        }
        for m in g.mounts(&c)? {
            let canonical = fs::canonicalize(m.host.as_ref().ok_or("share has no host source")?)?;
            if !g
                .shares
                .values()
                .any(|share| canonical.starts_with(&share.host))
            {
                return Err("mount source resolves outside host grants".into());
            }
            args.extend([
                if m.readonly { "--mount-ro" } else { "--mount" }.into(),
                format!(
                    "{}={}",
                    m.target,
                    canonical.to_str().ok_or("share path must be UTF-8")?
                ),
            ]);
        }
        if c.display.windows && c.interop.host_ui.enabled && g.host_ui {
            if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) {
                args.extend(["--desktop".into(), "wayland".into()]);
            } else if std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty()) {
                args.extend(["--desktop".into(), "x11".into()]);
            }
        }
        args.push("--".into());
        if c.boot.mode == model::Init::Systemd {
            args.push("/sbin/init".into());
        } else {
            args.extend([
                "/bin/sh".into(),
                "-c".into(),
                trames_distro::terminals::launcher_for_session(&c.session),
            ]);
        }
        service = Some(crate::managed_io::Service::open(
            &r.socket(n)?,
            ready.take(),
            notifier.take(),
        )?);
        let result = crate::foreground::run_managed(
            args,
            &dir,
            &c,
            service.as_mut().unwrap(),
            c.interop.clipboard.enabled && g.clipboard,
        );
        let result = if let Some(error) = service.as_mut().unwrap().ready_error.take() {
            Err(error.into())
        } else {
            result
        };
        let exit = serde_json::json!({"code":result.as_ref().copied().unwrap_or(2),"error":result.as_ref().err().map(ToString::to_string)});
        registry::atomic_write(
            &dir.join("exit.json"),
            serde_json::to_string(&exit)?.as_bytes(),
        )?;
        service.as_mut().unwrap().retire();
        drop(_owner);
        result
    })();
    if let Some(service) = &mut service {
        service.finish(&result);
    } else {
        crate::managed_io::startup_error(
            &mut ready,
            &result
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_default(),
        );
    }
    result
}
fn data_command(r: &Registry, op: &str, args: &[String]) -> Result<()> {
    super::environment_data::dispatch(r, op, args)
}

fn explicit_storage(args: &[String]) -> bool {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "--" || !arg.starts_with('-') {
            break;
        }
        if matches!(arg.as_str(), "--rootfs" | "--store" | "--socket") {
            return true;
        }
        if matches!(
            arg.as_str(),
            "--setenv"
                | "--cwd"
                | "-w"
                | "--cpus"
                | "--timeout-ms"
                | "--network"
                | "--desktop"
                | "--desktop-renderer"
                | "--vulkan-icd"
                | "--xwayland-launcher"
                | "--display"
                | "--display-size"
                | "--memory-mib"
                | "--mount"
                | "--mount-ro"
                | "--sysctl"
                | "--network-link"
                | "--network-name"
                | "--network-id"
                | "--network-nat"
                | "--publish"
        ) {
            i += 1;
        }
        i += 1;
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_store_selection_stops_at_literal_command_and_skips_values() {
        let args = |s: &[&str]| s.iter().map(|v| v.to_string()).collect::<Vec<_>>();
        assert!(!explicit_storage(&args(&["/bin/echo", "--rootfs"])));
        assert!(!explicit_storage(&args(&[
            "--setenv",
            "--rootfs",
            "--",
            "/bin/echo"
        ])));
        assert!(!explicit_storage(&args(&["--", "--rootfs"])));
        assert!(explicit_storage(&args(&[
            "--cwd",
            "/tmp",
            "--rootfs",
            "/image",
            "--",
            "/bin/echo"
        ])));
    }
}
