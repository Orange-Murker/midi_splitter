use std::io::{Cursor, Write};

use futures_channel::oneshot;
use leptos::*;
use midly::{num::u7, MetaMessage, MidiMessage, Smf, TrackEventKind};
use web_sys::{
    js_sys::{Array, Uint8Array},
    wasm_bindgen::{closure::Closure, JsCast},
    Blob, BlobPropertyBag, Url,
};
use zip::{write::FileOptions, ZipWriter};

struct File {
    name: String,
    data: Vec<u8>,
}

/// Load the file from the input element
async fn load_file(file_input: HtmlElement<html::Input>) -> File {
    let file_reader = web_sys::FileReader::new().expect("FileReader not supported");
    let file_reader_2 = file_reader.clone();
    let (sender, receiver) = oneshot::channel();
    let mut sender = Some(sender);

    let on_file_upload: Closure<dyn FnMut()> = Closure::new(move || {
        let result_blob = file_reader_2.result().expect("Failed to read file");
        let result_vec = Uint8Array::new(&result_blob).to_vec();
        sender
            .take()
            .expect("Could not take the channel. Closure called twice")
            .send(result_vec)
            .expect("Failed to send file from the callback");
    });

    let file = file_input
        .files()
        .expect("No files")
        .item(0)
        .expect("No files");
    file_reader.set_onload(Some(on_file_upload.as_ref().unchecked_ref()));
    on_file_upload.forget();
    file_reader
        .read_as_array_buffer(&file)
        .expect("Failed to read file");

    let name = file.name();
    let data = receiver
        .await
        .expect("Failed to receive file from the callback");

    File { name, data }
}

struct MidiProcessResult {
    zip_name: String,
    file_names: Vec<String>,
    zip_file: Vec<u8>,
}

/// Write the given smf to the zip file
fn write_midi_file_to_zip(
    zip: &mut ZipWriter<Cursor<Vec<u8>>>,
    smf: &Smf,
    file_name: &str,
) -> anyhow::Result<()> {
    let mut midi_file: Vec<u8> = Vec::new();
    smf.write(&mut midi_file)
        .map_err(|e| anyhow::anyhow!("Failed to write midi file: {}", e))?;

    zip.start_file(file_name, FileOptions::default())?;
    zip.write_all(&midi_file)?;

    Ok(())
}

/// Reduce note velocities for a given file
fn process_file(file: File, velocity_reduction: u8) -> anyhow::Result<MidiProcessResult> {
    let (file_name, extension) = file
        .name
        .rsplit_once(".")
        .ok_or(anyhow::anyhow!("No file extension"))?;

    let smf = Smf::parse(&file.data)?;

    let zip_file: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    let mut file_names: Vec<String> = Vec::new();

    let mut zip = ZipWriter::new(zip_file);

    for i in 0..smf.tracks.len() {
        // Clone the smf so we can modify it
        let mut track_smf = smf.clone();
        let current_track = &track_smf.tracks[i];

        let mut track_name: Option<&str> = None;

        // Find the track name
        for event in current_track {
            match event.kind {
                TrackEventKind::Meta(meta) => match meta {
                    MetaMessage::TrackName(name) => {
                        track_name = Some(std::str::from_utf8(&name)?);
                        break;
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        // Reduce the velocity for all tracks except the current one
        for (index, track) in track_smf.tracks.iter_mut().enumerate() {
            if index == i {
                continue;
            }

            for event in track {
                match &mut event.kind {
                    TrackEventKind::Midi {
                        channel: _,
                        message,
                    } => match message {
                        MidiMessage::NoteOn { key: _, vel } => {
                            *vel = vel.as_int().saturating_sub(velocity_reduction).into();
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }

        let default_track_name = format!("track-{}", i);
        let track_name = track_name.unwrap_or(&default_track_name);

        let name = format!("{}_{}.{}", file_name, track_name, extension);
        file_names.push(name.clone());

        write_midi_file_to_zip(&mut zip, &track_smf, &name)?;
    }

    let name = format!("{}_All.{}", file_name, extension);
    write_midi_file_to_zip(&mut zip, &smf, &name)?;
    file_names.push(name);

    Ok(MidiProcessResult {
        zip_name: file_name.to_string(),
        file_names,
        zip_file: zip.finish()?.into_inner(),
    })
}

#[component]
fn App() -> impl IntoView {
    let file_input_ref: NodeRef<html::Input> = create_node_ref();
    let (error, set_error) = create_signal(None::<String>);
    let (number_error, set_number_error) = create_signal(None::<String>);

    let (velocity_reduction, set_velocity_reduction) = create_signal(30);

    let (file_url, set_file_url) = create_signal(None::<String>);
    let (zip_name, set_zip_name) = create_signal(None::<String>);
    let (file_names, set_file_names) = create_signal(Vec::new());

    let process_file_action = create_action(move |_| async move {
        let file_input = file_input_ref.get_untracked().expect("<input> not mounted");

        let file = load_file(file_input).await;
        let process_result = process_file(file, velocity_reduction.get_untracked());
        let process_result = match process_result {
            Ok(process_result) => {
                set_error(None);
                process_result
            }
            Err(e) => {
                set_error(Some(e.to_string()));
                return;
            }
        };

        let u8array = Uint8Array::from(process_result.zip_file.as_slice());
        let array = Array::new();
        array.push(&u8array.buffer());
        let blob = Blob::new_with_u8_array_sequence_and_options(
            &array,
            &BlobPropertyBag::new().type_("application/zip"),
        )
        .expect("Failed to create blob from MIDI file");
        let url = Url::create_object_url_with_blob(&blob).expect("Failed to create object URL");
        set_zip_name(Some(process_result.zip_name));
        set_file_names(process_result.file_names);
        set_file_url(Some(url));
    });

    view! {
        <div class="min-h-screen p-10 flex flex-col items-center gap-4 bg-slate-800 text-slate-200">
            <p class="text-lg mb-4">
                Create files for each MIDI track with reduced note velocities for other tracks
            </p>
            {move || {
                error()
                    .map(|error| {
                        view! {
                            <div id="error" class="w-full bg-red-500 p-4 rounded">
                                <p class="text-lg">Error</p>
                                <p class="text-sm">{error}</p>
                            </div>
                        }
                    })
            }}

            {move || {
                number_error()
                    .map(|error| {
                        view! {
                            <div id="error" class="w-full bg-red-500 p-4 rounded">
                                <p class="text-lg">Number Error</p>
                                <p class="text-sm">{error}</p>
                            </div>
                        }
                    })
            }}

            <div class="flex flex-col gap-2">
                <label class="mb-2 text-sm font-medium" for="vol_input">
                    Reduce the note velocities by (0-127)
                </label>
                <input
                    class="border-2 rounded p-2 text-slate-900"
                    id="vol_input"
                    type="number"
                    min="0"
                    max="127"
                    on:input=move |ev| {
                        let value = event_target_value(&ev);
                        match value.parse::<u8>() {
                            Ok(value) => {
                                if value > u7::max_value() {
                                    set_number_error(
                                        Some(
                                            "The number entered is too large. Must be between 0 and 127"
                                                .to_string(),
                                        ),
                                    );
                                    return;
                                }
                                set_number_error(None);
                                set_velocity_reduction(value);
                            }
                            Err(_) => {
                                set_number_error(
                                    Some(
                                        "Invalid number entered for note velocity reduction"
                                            .to_string(),
                                    ),
                                )
                            }
                        }
                    }

                    prop:value=velocity_reduction
                />
            </div>

            <div class="w-full flex flex-col">
                <label class="mb-2 text-sm font-medium" for="file_input">
                    Upload file
                </label>
                <input
                    class="border-2 rounded p-2 cursor-pointer"
                    id="file_input"
                    type="file"
                    node_ref=file_input_ref
                    on:change=move |_ev| {
                        if number_error().is_some() {
                            set_error(
                                Some(
                                    "Cannot process file until a valid number is entered"
                                        .to_string(),
                                ),
                            );
                            return;
                        }
                        set_error(None);
                        process_file_action.dispatch("");
                    }
                />

            </div>

            {move || {
                if file_names().len() > 0 {
                    Some(
                        view! {
                            <div
                                class="flex flex-col gap-2 p-4 border-2"
                                hidden=move || file_names().len() == 0
                            >
                                <p class="text-lg mb-2">The following files have been created:</p>
                                <For
                                    each=file_names
                                    key=|file_name| file_name.clone()
                                    children=|file_name| {
                                        view! { <p class="text-m">{file_name}</p> }
                                    }
                                />

                            </div>
                        },
                    )
                } else {
                    None
                }
            }}

            {move || {
                file_url()
                    .map(|url| {
                        Some(
                            view! {
                                <a
                                    class="bg-blue-500 hover:bg-blue-700 font-bold p-4 rounded"
                                    href=url
                                    download=zip_name
                                >
                                    Download
                                </a>
                            },
                        )
                    })
            }}

        </div>
    }
}

fn main() {
    console_error_panic_hook::set_once();

    mount_to_body(|| view! { <App/> })
}
