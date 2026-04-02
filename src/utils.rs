use std::env;
use std::path::Path;
use std::process::{Command, Stdio};

/// Returns true if the current process is running as root (UID 0).
pub fn is_root() -> bool {
    unsafe { libc::getuid() == 0 }
}

/// Returns true if the path cannot be read without elevated privileges.
pub fn path_needs_root(path: &Path) -> bool {
    if is_root() { return false; }
    matches!(
        std::fs::read_dir(path),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied
    )
}

/// Relaunch the current binary via pkexec (PolicyKit).
pub fn relaunch_with_pkexec() -> anyhow::Result<()> {
    let exe  = env::current_exe()?;
    let args: Vec<String> = env::args().skip(1).collect();
    let status = Command::new("pkexec")
        .arg(exe).args(&args)
        .stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        anyhow::bail!("pkexec returned non-zero: {:?}", status.code());
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SudoChoice { Limited, Sudo, Cancel }

/// Show a gtk4::AlertDialog asking how to proceed with a protected path.
/// Spins a nested GLib main loop until the user responds.
/// Returns `true`  → continue with limited scan
/// Returns `false` → cancel / relaunching via pkexec
pub fn show_sudo_dialog() -> bool {
    use std::cell::Cell;
    use std::rc::Rc;

    let choice = Rc::new(Cell::new(SudoChoice::Cancel));
    let done   = Rc::new(Cell::new(false));

    // gtk4 0.8: AlertDialog uses .message() / .detail() / .buttons()
    let dialog = gtk4::AlertDialog::builder()
        .message("Elevated Access Required")
        .detail(
            "Some directories require root access (e.g. /etc, /root, /boot).\n\n\
             Continue with a limited scan, or restart via pkexec (PolicyKit)."
        )
        .buttons(["Cancel", "Continue (limited scan)", "Restart with sudo"])
        .cancel_button(0)
        .default_button(1)
        .modal(true)
        .build();

    let choice_c = choice.clone();
    let done_c   = done.clone();
    dialog.choose(gtk4::Window::NONE, gtk4::gio::Cancellable::NONE, move |result| {
        choice_c.set(match result {
            Ok(1) => SudoChoice::Limited,
            Ok(2) => SudoChoice::Sudo,
            _     => SudoChoice::Cancel,
        });
        done_c.set(true);
    });

    let ctx = glib::MainContext::default();
    while !done.get() { ctx.iteration(true); }

    match choice.get() {
        SudoChoice::Limited => true,
        SudoChoice::Sudo => {
            match relaunch_with_pkexec() {
                Ok(_)  => false,
                Err(e) => {
                    eprintln!("pkexec failed: {}", e);
                    show_error_dialog(
                        "Could not elevate privileges",
                        "pkexec was not found or the request was denied.\n\
                         Run manually with:    sudo hashmyfiles",
                    );
                    true
                }
            }
        }
        SudoChoice::Cancel => false,
    }
}

fn show_error_dialog(message: &str, detail: &str) {
    use std::cell::Cell;
    use std::rc::Rc;

    let done = Rc::new(Cell::new(false));
    let dialog = gtk4::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .buttons(["OK"])
        .default_button(0)
        .modal(true)
        .build();

    let done_c = done.clone();
    dialog.choose(gtk4::Window::NONE, gtk4::gio::Cancellable::NONE,
        move |_| { done_c.set(true); });

    let ctx = glib::MainContext::default();
    while !done.get() { ctx.iteration(true); }
}
