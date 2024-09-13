use std::thread::JoinHandle;
use std::{cmp::Ordering, collections::BTreeSet, fs::File, path::PathBuf};
use strum::Display;

// FIXME: Temporary rodio playback, might need to use cpal or make rodio proper
use rodio::{Decoder, OutputStream, Sink};
use open::that_detached;
use egui::{
    include_image, pos2, vec2, Align2, Context, FontFamily, FontId, Image, PointerButton, Pos2, Rect, Stroke, Ui, LayerId
};

use std::io::BufReader;

use unicode_truncate::UnicodeTruncateStr;

use crate::visual::ThemeColors;

fn hovered(ctx: &Context, rect: &Rect) -> bool {
    ctx.rect_contains_pointer(
        ctx.layer_id_at(ctx.pointer_hover_pos().unwrap_or_default())
            .unwrap_or_else(LayerId::background),
        *rect,
    )
}

#[derive(Display, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCategory {
    Files,
    Devices,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserEntry {
    pub path: PathBuf,
    pub kind: BrowserEntryKind,
}

impl Ord for BrowserEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.kind.cmp(&other.kind).then(
            self.path
                .file_name()
                .unwrap()
                .cmp(other.path.file_name().unwrap()),
        )
    }
}

impl PartialOrd for BrowserEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Display, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BrowserEntryKind {
    Directory,
    Audio,
    File,
}

pub struct Preview {
    pub preview_thread: Option<JoinHandle<()>>
}

impl Preview {
    pub fn play_file(&mut self, file: File) {
        // Kill the current thread if it's not sleeping
        if let Some(thread) = self.preview_thread.take() {
            if !thread.is_finished() {
                thread.thread().unpark();
            }
        }

        let file = BufReader::new(file);
        self.preview_thread = Some(std::thread::spawn(move || {
            let (_stream, stream_handle) = OutputStream::try_default().unwrap();
            let source = Decoder::new(file).unwrap();
            let sink = Sink::try_new(&stream_handle).unwrap();
            // let source = SineWave::new(440.0).take_duration(Duration::from_secs_f32(0.25)).amplify(0.20);
            sink.append(source);
            std::thread::park();
        }));
    }
}

pub struct Browser {
    pub entries: BTreeSet<BrowserEntry>,
    pub selected_category: BrowserCategory,
    pub path: PathBuf,
    pub preview: Preview,
    pub offset_y: f32,
    pub began_scroll: bool
}

impl Browser {
    pub fn paint_button(
        ctx: &Context,
        ui: &Ui,
        button: &Rect,
        selected: bool,
        text: &str,
        theme: &ThemeColors,
    ) {
        let color = if selected {
            theme.browser_selected_button_fg
        } else if hovered(ctx, button) {
            theme.browser_unselected_hover_button_fg
        } else {
            theme.browser_unselected_button_fg
        };
        ui.painter().text(
            button.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::new(14.0, FontFamily::Name("IBMPlexMono".into())),
            color,
        );
        ui.painter().line_segment(
            [
                Pos2 {
                    x: button.left() + 8.,
                    y: button.bottom(),
                },
                Pos2 {
                    x: button.right() - 8.,
                    y: button.bottom(),
                },
            ],
            Stroke::new(0.5, color),
        );
    }

    pub fn paint(&mut self, ctx: &Context, ui: &mut Ui, viewport: &Rect, theme: &ThemeColors) {
        ui.painter().rect_filled(
            Rect {
                min: Pos2 { x: 0., y: 50. },
                max: Pos2 {
                    x: 300.,
                    y: viewport.height(),
                },
            },
            0.0,
            theme.browser,
        );
        ui.painter().line_segment(
            [
                Pos2 { x: 300., y: 50. },
                Pos2 {
                    x: 300.,
                    y: viewport.height(),
                },
            ],
            Stroke::new(0.5, theme.browser_outline),
        );
        let (was_pressed, press_position) = ctx
            .input(|input_state| {
                Some((
                    input_state.pointer.button_released(PointerButton::Primary),
                    Some(input_state.pointer.latest_pos()?),
                ))
            })
            .unwrap_or((false, None));
        for (category, rect) in [
            (
                BrowserCategory::Files,
                Rect::from_min_size(pos2(0., 55.), vec2(150., 30.)),
            ),
            (
                BrowserCategory::Devices,
                Rect::from_min_size(pos2(150., 55.), vec2(150., 30.)),
            ),
        ] {
            let open = self.selected_category == category;
            Self::paint_button(ctx, ui, &rect, open, category.to_string().as_str(), theme);
            if press_position
                .is_some_and(|press_position| was_pressed && rect.contains(press_position))
            {
                self.selected_category = category;
            }
        }
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        if let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) {
            if pos.x <= 300. && !self.began_scroll && scroll != 0. {
                self.began_scroll = true;
            }
        }
        if self.began_scroll && scroll != 0. {
            self.offset_y += scroll;
        }
        if self.began_scroll && scroll == 0. {
            self.began_scroll = false;
        }
        match self.selected_category {
            BrowserCategory::Files => {
                // Add ".." entry if not at root
                let entries_ref = self.entries.iter().collect::<Vec<_>>();
                let mut entries: Vec<BrowserEntry>;
                if self.path != std::path::Path::new("/") {
                    let b = BrowserEntry {
                        path: self.path.parent().unwrap_or(&self.path).to_path_buf(),
                        kind: BrowserEntryKind::Directory,
                    };
                    entries = vec![b.clone()];
                    entries.extend(entries_ref.iter().map(|&e| e.clone()));
                } else {
                    entries = entries_ref.iter().map(|&e| e.clone()).collect();
                }

                // Calculate the maximum offset based on the number of entries and browser height
                let max_entries = entries.len();
                let browser_height = viewport.height() - 90.0; // Adjust for header height
                let bottom_margin = 8.0; // Add a slight margin at the bottom
                let max_offset = (max_entries as f32 * 16.0) - browser_height + bottom_margin;

                // Clamp the offset
                self.offset_y = self.offset_y.clamp(-max_offset.max(0.0), 0.0);

                for (index, entry) in entries.iter().enumerate() {
                    #[allow(clippy::cast_precision_loss)]
                    let y = (index as f32).mul_add(16.0, 90.);
                    let rect = &Rect::from_min_size(pos2(0., y + self.offset_y), vec2(300., 16.));
                    egui::Frame::none()
                        .show(ui, |ui| {
                            ui.allocate_space(ui.available_size());
                            let name = if entry.path == self.path.parent().unwrap_or(&self.path) {
                                ".."
                            } else {
                                entry.path.file_name().unwrap().to_str().unwrap()
                            };
                            if y + self.offset_y >= 90. {
                                ui.painter().text(
                                    pos2(30., y + self.offset_y),
                                    Align2::LEFT_TOP,
                                    if name.to_string().unicode_truncate(30).1 == 30 {
                                        name.to_string().unicode_truncate(30).0.to_string() + "..."
                                    } else {
                                        name.to_string()
                                    },
                                    FontId::new(14., FontFamily::Name("IBMPlexMono".into())),
                                    if hovered(ctx, rect) {
                                        theme.browser_unselected_hover_button_fg
                                    } else {
                                        theme.browser_unselected_button_fg
                                    },
                                )
                            } else {
                                Rect {min: Pos2 { x: 0., y: 0. }, max: Pos2 { x: 0., y: 0. }}
                            }
                        });

                    if y + self.offset_y >= 90. {
                        Image::new(match entry.kind {
                            BrowserEntryKind::Directory => include_image!("images/icons/folder.png"),
                            BrowserEntryKind::Audio => include_image!("images/icons/audio.png"),
                            BrowserEntryKind::File => include_image!("images/icons/file.png"),
                        })
                        .paint_at(ui, Rect::from_min_size(pos2(10., y + 2. + self.offset_y), vec2(14., 14.)));
                    }
                    if press_position
                        .is_some_and(|press_position| was_pressed && rect.contains(press_position))
                        && press_position.unwrap().y >= 90.
                    {
                        match entry.kind {
                            BrowserEntryKind::Directory => {
                                if entry.path == self.path.parent().unwrap_or(&self.path) {
                                    self.path = self.path.parent().unwrap_or(&self.path).to_path_buf();
                                } else {
                                    self.path.clone_from(&entry.path);
                                }
                                break;
                            }
                            BrowserEntryKind::Audio => {
                                // TODO: Proper preview implementation with cpal. This is temporary (or at least make it work well with a proper preview widget)
                                // Also, don't spawn a new thread - instead, dedicate a thread for preview
                                let file = File::open(entry.path.as_path()).unwrap();
                                self.preview.play_file(file);
                            }
                            BrowserEntryKind::File => {
                                that_detached(entry.path.clone()).unwrap();
                            }
                        }
                    }
                }
            }
            BrowserCategory::Devices => {
                // TODO: Show some devices here!
            }
        }
    }
}