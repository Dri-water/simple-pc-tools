// Release builds have no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod autostart;
mod desktop;
mod elevation;
mod instance;
mod sandbox;
mod settings;
mod tools;
mod update;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

use crate::settings::SettingsFile;

const HELP: &str = "\
simple-pc-tools [options]

  (no options)        open the window (starts the app if it isn't running)
  --background        start without opening the window (used at sign-in)
  --data-dir DIR      where settings and logs live (default %LOCALAPPDATA%\\Simple PC Tools)
  --instance-port N   local port that keeps one copy running (default 47615)
  --no-elevate        don't ask for administrator rights (development)
  --dev-server ADDR   also serve the UI over HTTP, e.g. 127.0.0.1:7901 (development)
  --setup [--tools apps,disk|none] [--autostart on|off]
                      installer step: save tool choices, set up Start with Windows
  --uninstall [--remove-data]
                      uninstaller step: quit, remove Start with Windows
  --quit              ask the running copy to quit
";

#[derive(Default)]
struct Args {
    raw: Vec<String>,
    data_dir: Option<PathBuf>,
    background: bool,
    port: Option<u16>,
    no_elevate: bool,
    dev_server: Option<String>,
    setup: bool,
    tools: Option<String>,
    autostart: Option<bool>,
    uninstall: bool,
    remove_data: bool,
    quit: bool,
}

fn parse_args() -> Result<Args> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut a = Args {
        raw: raw.clone(),
        ..Default::default()
    };
    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--background" => a.background = true,
            "--data-dir" => {
                a.data_dir = Some(it.next().context("--data-dir needs a folder")?.into())
            }
            "--instance-port" => {
                a.port = Some(
                    it.next()
                        .context("--instance-port needs a number")?
                        .parse()?,
                )
            }
            "--no-elevate" => a.no_elevate = true,
            "--dev-server" => {
                a.dev_server = Some(it.next().context("--dev-server needs an address")?)
            }
            "--setup" => a.setup = true,
            "--tools" => a.tools = Some(it.next().context("--tools needs a list")?),
            "--autostart" => {
                a.autostart = Some(match it.next().as_deref() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => anyhow::bail!("--autostart needs on or off"),
                })
            }
            "--uninstall" => a.uninstall = true,
            "--remove-data" => a.remove_data = true,
            "--quit" => a.quit = true,
            // Passed by the updater to the installer, and back to us; nothing to do.
            "--update" => {}
            "-h" | "--help" | "/?" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown option {other:?}\n\n{HELP}"),
        }
    }
    Ok(a)
}

fn default_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Simple PC Tools")
}

/// Shows a plain Windows message (for when there's no window to show it in).
fn message_box(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, title) = (wide(text), wide("Simple PC Tools"));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONWARNING,
        )
    };
}

/// Tool choices an installer couldn't save directly (see `setup`). ProgramData
/// is outside the folders a sandbox redirects.
fn pending_setup_file() -> Option<PathBuf> {
    std::env::var_os("PROGRAMDATA")
        .map(|d| PathBuf::from(d).join("Simple PC Tools").join("setup.json"))
}

fn apply_pending_setup(settings: &SettingsFile) {
    let Some(file) = pending_setup_file() else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(&file) else {
        return;
    };
    match serde_json::from_str::<std::collections::BTreeMap<String, bool>>(&text) {
        Ok(choices) => match settings.update(|s| s.tools.extend(choices)) {
            Ok(_) => tracing::info!("applied the tool choices saved by the installer"),
            Err(e) => return tracing::error!("saving the installer's tool choices: {e:#}"),
        },
        Err(e) => tracing::error!("{} unreadable ({e}); ignored", file.display()),
    }
    let _ = std::fs::remove_file(&file);
    if let Some(dir) = file.parent() {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Brings back an app list that a copy started inside another app's sandbox
/// saved in that app's private folder (see `sandbox`), if this copy has none.
fn recover_stranded_apps(data_dir: &std::path::Path) {
    use tools::apps::{store_path, Store};
    let ours = store_path(data_dir);
    if Store::load(&ours).is_ok_and(|s| !s.apps.is_empty()) {
        return;
    }
    for dir in sandbox::stranded(data_dir, "apps.json") {
        let theirs = dir.join("apps.json");
        let Ok(store) = Store::load(&theirs) else {
            continue;
        };
        if store.apps.is_empty() {
            continue;
        }
        if ours.exists() {
            let _ = std::fs::copy(&ours, ours.with_extension("json.before-recovery"));
        }
        if let Err(e) = store.save(&ours) {
            tracing::error!("bringing back the app list from {}: {e:#}", dir.display());
            return;
        }
        let settings = data_dir.join("settings.json");
        if !settings.exists() {
            let _ = std::fs::copy(dir.join("settings.json"), &settings);
        }
        // Keep it, renamed, so it isn't brought back again.
        let _ = std::fs::rename(&theirs, theirs.with_extension("json.recovered"));
        tracing::warn!(
            "brought back {} apps saved inside another app's sandbox ({})",
            store.apps.len(),
            dir.display()
        );
        return;
    }
}

fn init_logging(data_dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_dir.join("simple-pc-tools.log"))?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "simple_pc_tools=info,disk_scan=info".into()),
        )
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .init();
    Ok(())
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let port = args.port.unwrap_or(instance::DEFAULT_PORT);
    let data_dir = args.data_dir.clone().unwrap_or_else(default_data_dir);

    if args.setup {
        return setup(&args, &data_dir);
    }
    if args.uninstall {
        let _ = instance::send(port, "QUIT");
        autostart::disable()?;
        if args.remove_data {
            let _ = std::fs::remove_dir_all(&data_dir);
        }
        return Ok(());
    }
    if args.quit {
        return instance::send(port, "QUIT").context("Simple PC Tools isn't running");
    }

    // Already running: bring its window up (unless this is the sign-in start).
    if instance::is_running(port) {
        if !args.background {
            instance::send(port, "SHOW")?;
        }
        return Ok(());
    }

    // Not elevated: hand over to the sign-in task (no UAC prompt), else ask once.
    if !args.no_elevate && !elevation::is_elevated() {
        if autostart::is_enabled() && autostart::run_now().is_ok() && instance::wait_for(port, 15) {
            if !args.background {
                instance::send(port, "SHOW")?;
            }
            return Ok(());
        }
        return elevation::relaunch_elevated(&args.raw);
    }

    // Started inside another app's sandbox (an MSIX app such as Claude
    // Desktop): Windows would keep this copy's app list and settings in that
    // app's private folder, where the copy that starts with Windows never
    // looks. Hand over to the sign-in task, which runs outside it. (Only for
    // the real copy: development copies pass their own folder and port.)
    if args.data_dir.is_none() && args.port.is_none() {
        if let Some(package) = sandbox::redirected(&data_dir) {
            if autostart::is_enabled()
                && autostart::run_now().is_ok()
                && instance::wait_for(port, 15)
            {
                if !args.background {
                    instance::send(port, "SHOW")?;
                }
                return Ok(());
            }
            // Package folders are "<name>_<publisher id>".
            let folder = package.file_name().unwrap_or_default().to_string_lossy();
            let from = folder.split('_').next().unwrap_or(&folder);
            message_box(&format!(
                "Simple PC Tools was started from inside another app ({from}). Windows \
                 would keep its settings and app list in that app's private folder, \
                 where the copy that starts with Windows can't see them.\n\n\
                 Open Simple PC Tools from the Start menu instead."
            ));
            anyhow::bail!("started inside {from}'s sandbox");
        }
    }

    let listener = match instance::claim(port) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Lost a race with another start; show that one.
            return instance::send(port, "SHOW");
        }
        Err(e) => return Err(e).with_context(|| format!("claiming local port {port}")),
    };
    init_logging(&data_dir)?;
    tracing::info!(
        "Simple PC Tools {} starting (elevated: {}, data: {})",
        update::VERSION,
        elevation::is_elevated(),
        data_dir.display()
    );

    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();
    let settings = Arc::new(SettingsFile::open(&data_dir));
    apply_pending_setup(&settings);
    recover_stranded_apps(&data_dir);
    let tools = tools::Tools::start(&data_dir, &settings.get());
    let updater = update::Updater::new();

    let desktop = desktop::Desktop::new();
    let handle = desktop.handle();
    let quit_handle = handle.clone();
    let app = api::App {
        settings: settings.clone(),
        updater: updater.clone(),
        tools,
        data_dir: data_dir.clone(),
        exe: std::env::current_exe()?,
        quit: Arc::new(move || quit_handle.quit()),
    };
    let router = api::router(app);

    let commands = handle.clone();
    instance::serve(listener, move |c| match c {
        instance::Command::Show => commands.show(),
        instance::Command::Quit => commands.quit(),
    });
    runtime.spawn(updater.run(settings.clone()));
    if let Some(addr) = &args.dev_server {
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind(addr))
            .with_context(|| format!("binding {addr}"))?;
        tracing::warn!("development server on http://{addr}/ (anyone on this PC can use it)");
        let router = router.clone();
        runtime.spawn(async move { axum::serve(listener, router).await });
    }

    let theme = settings.clone();
    let dark = move || match theme.get().theme.as_str() {
        "dark" => true,
        "light" => false,
        _ => desktop::system_uses_dark_theme(),
    };
    let result = desktop.run(
        router,
        runtime.handle().clone(),
        data_dir,
        !args.background,
        dark,
    );
    drop(runtime);
    result
}

/// Installer step: saves the chosen tools and sets up Start with Windows.
fn setup(args: &Args, data_dir: &std::path::Path) -> Result<()> {
    if let Some(list) = &args.tools {
        let chosen: Vec<&str> = list.split(',').map(str::trim).collect();
        if let Some(bad) = chosen
            .iter()
            .find(|&&t| t != "none" && tools::find(t).is_none())
        {
            anyhow::bail!("unknown tool {bad:?}");
        }
        let choices: std::collections::BTreeMap<String, bool> = tools::ALL
            .iter()
            .map(|t| (t.id.to_string(), chosen.contains(&t.id)))
            .collect();
        // An installer started from inside another app's sandbox can't reach
        // the real settings file; leave the choices where the app picks them
        // up at its next start (outside the sandbox, from the sign-in task).
        match (sandbox::redirected(data_dir), pending_setup_file()) {
            (Some(_), Some(file)) => settings::write_atomic(&file, &serde_json::to_vec(&choices)?)?,
            _ => {
                SettingsFile::open(data_dir).update(|s| s.tools.extend(choices))?;
            }
        }
    }
    match args.autostart {
        Some(true) => autostart::enable(&std::env::current_exe()?)?,
        Some(false) => autostart::disable()?,
        None => {}
    }
    Ok(())
}
