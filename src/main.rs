use lopi::error::Abort;
use lopi::output;
use lopi::ui::tty;

fn main() {
    // ssh started us as its askpass program: answer the prompt and nothing else.
    if let Some(code) = lopi::askpass::run_if_requested() {
        std::process::exit(code);
    }
    // Checked before running: once ssh starts it shares the console.
    let double_clicked = tty::launched_by_double_click();
    let code = match lopi::app::run() {
        Ok(code) => code,
        // The reader closed the pipe (`lopi list | head -1`): nothing left to say.
        Err(err) if output::is_broken_pipe(&err) => 0,
        Err(err) => match err.downcast_ref::<Abort>() {
            Some(abort) => {
                eprintln!("lopi: {abort}");
                abort.exit_code()
            }
            None => {
                eprintln!("lopi: error: {err:#}");
                1
            }
        },
    };
    if double_clicked {
        tty::wait_for_enter();
    }
    std::process::exit(code);
}
