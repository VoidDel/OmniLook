//! M0 probe for the hotkey and the Explorer selection (`docs/development-plan.md` §10 M0).
//!
//! ```text
//! probe tabs                 every Explorer tab, which one is in front, its selection
//! probe hook [space] [LOG]   log every Space / Escape / arrow key the hook sees, with its
//!                            decision; on the hotkey, read the selection and time it
//! ```
//!
//! `hook` pretends a preview opens on the hotkey, so Escape and arrow keys behave as they will.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    use std::{io::Write, sync::mpsc, time::Instant};

    use omnilook_win::{
        explorer::{self, Com, View},
        hook,
        trigger::{Decision, Hotkey},
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    let _com = Com::init()?;
    match args.first().map(String::as_str) {
        Some("tabs") => {
            for t in explorer::tabs()? {
                println!(
                    "window {:?} tab {:?} [{}] visible={} front={}\n  folder {:?}\n  selected {:?} (+{} virtual)\n  focused {:?}",
                    t.top.0,
                    t.tab.0,
                    t.tab_class,
                    t.tab_visible,
                    t.front,
                    t.snapshot.folder,
                    t.snapshot.selected,
                    t.snapshot.selected_virtual,
                    t.snapshot.focused
                );
            }
            match View::desktop().and_then(|v| v.snapshot()) {
                Ok(s) => println!("desktop: selected {:?}, focused {:?}", s.selected, s.focused),
                Err(e) => println!("desktop: {e}"),
            }
        }
        Some("hook") => {
            if args.iter().any(|a| a == "space") {
                hook::set_hotkey(Hotkey::Space);
            }
            let mut log: Box<dyn Write> = match args.iter().skip(1).find(|a| *a != "space") {
                Some(p) => Box::new(std::fs::File::create(p).expect("log file")),
                None => Box::new(std::io::stdout()),
            };
            let (tx, rx) = mpsc::channel();
            let _hook = hook::install(move |e| {
                let _ = tx.send(e);
            })?;
            let start = Instant::now();
            let mut open = false;
            writeln!(log, "hook installed; Ctrl+C to stop").ok();
            for e in rx {
                let f = &e.focus;
                writeln!(
                    log,
                    "{:>8} ms {:?} → {:?}  [{} | focus {} in {}] file_view={} mods={:?}",
                    e.at.duration_since(start).as_millis(),
                    e.key,
                    e.decision,
                    f.foreground_class,
                    f.focus_class,
                    f.parent_class,
                    f.in_file_view,
                    e.mods
                )
                .ok();
                let read = match e.decision {
                    Decision::Toggle => {
                        open = !open;
                        hook::set_preview_open(open);
                        open
                    }
                    Decision::Close => {
                        open = false;
                        hook::set_preview_open(false);
                        false
                    }
                    // Let Explorer move the selection first.
                    Decision::Follow => {
                        std::thread::sleep(std::time::Duration::from_millis(30));
                        true
                    }
                    Decision::Pass => false,
                };
                if read {
                    let t0 = Instant::now();
                    match View::of(f) {
                        Ok(Some(v)) => {
                            let t1 = Instant::now();
                            let s = v.snapshot();
                            let t2 = Instant::now();
                            let n = v.items().map(|i| i.len());
                            let t3 = Instant::now();
                            writeln!(
                                log,
                                "           tab {:?} front {:?}: view {} µs, snapshot {} µs, {:?} items {} µs\n           {:?}",
                                v.tab.0,
                                explorer::front_tab(f.window()).map(|h| h.0),
                                (t1 - t0).as_micros(),
                                (t2 - t1).as_micros(),
                                n,
                                (t3 - t2).as_micros(),
                                s
                            )
                            .ok();
                        }
                        Ok(None) => {
                            writeln!(log, "           no view").ok();
                        }
                        Err(err) => {
                            writeln!(log, "           error {err}").ok();
                        }
                    }
                    writeln!(
                        log,
                        "           hook → read done {} µs",
                        e.at.elapsed().as_micros()
                    )
                    .ok();
                }
                log.flush().ok();
            }
        }
        _ => eprintln!("usage: probe tabs | probe hook [space] [LOG]"),
    }
    Ok(())
}
