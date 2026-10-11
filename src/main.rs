#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod capture;
mod compare;
mod gallery;
mod probe;
mod worker;
use ferrite_design::prelude::*;
use gpui::StyledImage;
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, KeyBinding, Render, RenderImage,
    Subscription, Task, Window, div, img, px, size,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
const PAGES: [&str; 8] = [
    "Controls",
    "Forms",
    "Data & input",
    "Navigation",
    "Charts",
    "ASCII",
    "Layout",
    "Motion",
];
type Preview = Option<Arc<RenderImage>>;
struct Shot {
    scheme: &'static ferrite_design::schemes::Scheme,
    old: Preview,
    new: Preview,
    diff: Preview,
    label: String,
}
fn preview(path: &Path) -> Preview {
    image::open(path)
        .ok()
        .map(|i| Arc::new(RenderImage::new([image::Frame::new(i.into_rgba8())])))
}
fn key(page: usize, scheme: &str, light: bool) -> String {
    format!(
        "{page}-{scheme}-{}.png",
        if light { "light" } else { "dark" }
    )
}
fn paths(page: usize, scheme: &str, light: bool) -> (PathBuf, PathBuf, PathBuf) {
    let root = capture::root();
    let key = key(page, scheme, light);
    (
        root.join("baselines").join(&key),
        root.join("captures").join(&key),
        root.join("diffs").join(&key),
    )
}
fn load(page: usize, light: bool) -> Result<Vec<Shot>, String> {
    let mut shots = Vec::new();
    for scheme in SCHEMES {
        let (old, new, diff) = paths(page, scheme.key, light);
        let label = if old.exists() && new.exists() {
            let a = image::open(&old).map_err(|e| e.to_string())?.into_rgba8();
            let b = image::open(&new).map_err(|e| e.to_string())?.into_rgba8();
            let delta = compare::compare(
                &a,
                &b,
                scheme
                    .palette(if light {
                        ferrite_design::tokens::Tone::Light
                    } else {
                        ferrite_design::tokens::Tone::Dark
                    })
                    .accent,
                8,
            );
            std::fs::create_dir_all(diff.parent().unwrap()).map_err(|e| e.to_string())?;
            delta.image.save(&diff).map_err(|e| e.to_string())?;
            format!(
                "{:.3}% CHANGED{}",
                100. * delta.changed as f64 / delta.total.max(1) as f64,
                if delta.resized {
                    " · SIZE CHANGED"
                } else {
                    ""
                }
            )
        } else if new.exists() {
            "NO BASELINE · REVIEW THEN ACCEPT".into()
        } else {
            "NOT CAPTURED".into()
        };
        shots.push(Shot {
            scheme,
            old: preview(&old),
            new: preview(&new),
            diff: if old.exists() && new.exists() {
                preview(&diff)
            } else {
                None
            },
            label,
        });
    }
    Ok(shots)
}
fn accept(page: usize, light: bool) -> Result<(), String> {
    let root = capture::root();
    std::fs::create_dir_all(root.join("baselines")).map_err(|e| e.to_string())?;
    let mut count = 0;
    for scheme in SCHEMES {
        let (old, new, _) = paths(page, scheme.key, light);
        if new.exists() {
            std::fs::copy(new, old).map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    if count == 0 {
        Err("Capture this page first".into())
    } else {
        Ok(())
    }
}
struct Snap {
    page: usize,
    light: bool,
    shots: Vec<Shot>,
    busy: bool,
    status: String,
    confirm: bool,
    job: Option<Task<()>>,
    palette: Entity<CommandPalette>,
    _appearance: Subscription,
}
impl Snap {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        let palette = cx.new(|cx| CommandPalette::new(window, cx));
        palette.update(cx, |p, cx| {
            let mut commands = Vec::new();
            for scheme in SCHEMES {
                commands.push(
                    command(format!("Scheme: {}", scheme.name))
                        .group("Theme")
                        .on_run(move |_, cx| theme::set_scheme(scheme, cx)),
                );
            }
            commands.push(
                command("Capture selected page in every scheme")
                    .group("Snap")
                    .on_run(move |_, cx| {
                        let _ = weak.update(cx, |s, cx| s.capture(false, cx));
                    }),
            );
            p.set_commands(commands, cx);
        });
        Self {
            page: 0,
            light: false,
            shots: load(0, false).unwrap_or_default(),
            busy: false,
            status: "Capture a page, review the images, then accept a baseline.".into(),
            confirm: false,
            job: None,
            palette,
            _appearance: theme::follow_system(window),
        }
    }
    fn reload(&mut self, cx: &mut Context<Self>) {
        match load(self.page, self.light) {
            Ok(shots) => self.shots = shots,
            Err(e) => self.status = e,
        };
        cx.notify();
    }
    fn capture(&mut self, all: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.confirm = false;
        let page = self.page;
        let light = self.light;
        self.status = "Preparing native gallery captures…".into();
        self.job = Some(cx.spawn(async move |this, cx| {
            let pages: Vec<usize> = if all { (0..8).collect() } else { vec![page] };
            let total = pages.len() * SCHEMES.len();
            let mut complete = 0;
            'capture: for page in pages {
                for scheme in SCHEMES {
                    let text = format!(
                        "CAPTURE {}/{} · {} · {}",
                        complete + 1,
                        total,
                        PAGES[page],
                        scheme.name
                    );
                    if this
                        .update(cx, |s, cx| {
                            s.status = text;
                            cx.notify();
                        })
                        .is_err()
                    {
                        break 'capture;
                    }
                    let (_, path, _) = paths(page, scheme.key, light);
                    if let Err(error) =
                        worker::run(move || capture::capture(page, scheme.key, light, &path)).await
                    {
                        let _ = this.update(cx, |s, cx| {
                            s.busy = false;
                            s.status = error;
                            s.reload(cx);
                        });
                        return;
                    }
                    complete += 1;
                }
            }
            let _ = this.update(cx, |s, cx| {
                s.busy = false;
                s.status =
                    format!("{complete} captures saved. Review before accepting a baseline.");
                s.reload(cx);
            });
        }));
        cx.notify();
    }
}
fn image_panel(title: &str, image: Preview) -> impl IntoElement {
    let mut pane = panel(title).flex_1().min_w_0();
    pane = if let Some(image) = image {
        pane.child(
            img(image)
                .w_full()
                .h(px(240.))
                .object_fit(gpui::ObjectFit::Contain),
        )
    } else {
        pane.child(
            div()
                .h(px(240.))
                .flex()
                .items_center()
                .justify_center()
                .child("No image"),
        )
    };
    pane
}
impl Render for Snap {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        probe::record("Snap ready · real gallery capture and baseline comparison".into());
        let menu = Button::new("commands")
            .icon(Icon::Menu)
            .ghost()
            .small()
            .on_click(cx.listener(|s, _, w, cx| s.palette.update(cx, |p, cx| p.toggle(w, cx))));
        let mut pages = div().flex().flex_wrap().gap_2();
        for (i, page) in PAGES.iter().enumerate() {
            pages = pages.child(
                Button::new(("page", i))
                    .label(*page)
                    .secondary()
                    .small()
                    .disabled(self.busy)
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.page = i;
                        s.confirm = false;
                        s.reload(cx);
                    })),
            );
        }
        let mut images = div().flex().flex_col().gap_4();
        for shot in &self.shots {
            images = images.child(
                panel(shot.scheme.name).meta(shot.label.clone()).child(
                    div()
                        .flex()
                        .when(window.viewport_size().width < px(900.), |d| d.flex_col())
                        .gap_2()
                        .child(image_panel("Baseline", shot.old.clone()))
                        .child(image_panel("Current", shot.new.clone()))
                        .child(image_panel("Accent differences", shot.diff.clone())),
                ),
            );
        }
        let mut actions = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                Button::new("mode")
                    .label(if self.light {
                        "LIGHT CAPTURES"
                    } else {
                        "DARK CAPTURES"
                    })
                    .secondary()
                    .disabled(self.busy)
                    .on_click(cx.listener(|s, _, _, cx| {
                        s.light = !s.light;
                        s.confirm = false;
                        s.reload(cx);
                    })),
            )
            .child(
                Button::new("capture")
                    .label("Capture page / all schemes")
                    .disabled(self.busy)
                    .on_click(cx.listener(|s, _, _, cx| s.capture(false, cx))),
            )
            .child(
                Button::new("all")
                    .label("Capture all 8 pages")
                    .secondary()
                    .disabled(self.busy)
                    .on_click(cx.listener(|s, _, _, cx| s.capture(true, cx))),
            )
            .child(
                Button::new("accept")
                    .label("Accept page baseline")
                    .secondary()
                    .disabled(self.busy)
                    .on_click(cx.listener(|s, _, _, cx| {
                        s.confirm = true;
                        cx.notify();
                    })),
            );
        if self.confirm {
            actions = actions
                .child(
                    Button::new("confirm")
                        .label("Replace this page's baselines")
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.confirm = false;
                            s.status = match accept(s.page, s.light) {
                                Ok(()) => "Baselines accepted for this page and appearance".into(),
                                Err(e) => e,
                            };
                            s.reload(cx);
                        })),
                )
                .child(
                    Button::new("cancel")
                        .label("Cancel")
                        .ghost()
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.confirm = false;
                            cx.notify();
                        })),
                );
        }
        window_frame().child(power_on_in(
            "power",
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(hsla(p.bg))
                .text_color(hsla(p.fg))
                .body(text::BASE)
                .on_action(cx.listener(|s, _: &TogglePalette, w, cx| {
                    s.palette.update(cx, |p, cx| p.toggle(w, cx))
                }))
                .child(self.palette.clone())
                .child(title_bar("Snap").child(menu))
                .child(
                    div()
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(pages)
                        .child(
                            div().display(Scale::X1, window).child(
                                format!(
                                    "{} / {}",
                                    PAGES[self.page],
                                    if self.light { "LIGHT" } else { "DARK" }
                                )
                                .to_uppercase(),
                            ),
                        )
                        .child(actions)
                        .child(self.status.clone()),
                )
                .child(
                    scroll_area("comparisons")
                        .flex_1()
                        .min_h_0()
                        .child(div().p_3().child(images)),
                )
                .child(
                    status_bar()
                        .left("FIXED DATE · REDUCED MOTION · RGB TOLERANCE 8")
                        .right("NATIVE GALLERY"),
                ),
        ))
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--gallery") {
        gallery::run();
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--capture") {
        let result = (|| {
            let page = args
                .get(i + 1)
                .and_then(|s| s.parse().ok())
                .ok_or("Use --capture PAGE_INDEX SCHEME OUTPUT.png [--light]")?;
            let scheme = args.get(i + 2).ok_or("Missing scheme")?;
            let out = args.get(i + 3).ok_or("Missing output path")?;
            capture::capture(
                page,
                scheme,
                args.iter().any(|a| a == "--light"),
                Path::new(out),
            )
        })();
        match result {
            Ok(()) => println!("Gallery captured"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.iter().any(|a| a == "--smoke") {
        let a = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
        let mut b = a.clone();
        b.put_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        assert_eq!(compare::compare(&a, &b, 0, 8).changed, 1);
        println!("Snap comparison smoke passed");
        return;
    }
    gpui_platform::application().run(|cx: &mut App| {
        ferrite_design::init(Appearance::Dark, cx);
        theme::apply_env(cx);
        cx.bind_keys([KeyBinding::new("ctrl-shift-p", TogglePalette, None)]);
        let options =
            chrome::remembered_window_options("snap", "Snap", size(px(1320.), px(900.)), cx);
        cx.open_window(options, |w, cx| {
            chrome::square_corners(w);
            chrome::remember_window("snap", w, cx);
            chrome::power_off_on_close(w, cx);
            cx.new(|cx| Snap::new(w, cx))
        })
        .expect("opening Snap");
        cx.activate(true);
    });
}
