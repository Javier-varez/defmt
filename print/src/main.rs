use std::{
    env, fs,
    io::{self, Read},
    path::PathBuf,
};

use anyhow::anyhow;
use structopt::StructOpt;

/// Prints defmt-encoded logs to stdout
#[derive(StructOpt)]
#[structopt(name = "defmt-print", version = version())]
struct Opts {
    #[structopt(short, parse(from_os_str))]
    elf: PathBuf,
    // may want to add this later
    // #[structopt(short, long)]
    // verbose: bool,
    // TODO add file path argument; always use stdin for now
}

const READ_BUFFER_SIZE: usize = 1024;

fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::from_args();
    let verbose = false;
    defmt_logger::init(verbose);

    let bytes = fs::read(&opts.elf)?;

    let table = defmt_elf2table::parse(&bytes)?.ok_or_else(|| anyhow!(".defmt data not found"))?;
    let locs = defmt_elf2table::get_locations(&bytes, &table)?;

    let locs = if table.indices().all(|idx| locs.contains_key(&(idx as u64))) {
        Some(locs)
    } else {
        log::warn!("(BUG) location info is incomplete; it will be omitted from the output");
        None
    };

    let mut buf = [0; READ_BUFFER_SIZE];
    let mut frames = vec![];

    let current_dir = env::current_dir()?;
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    loop {
        let n = stdin.read(&mut buf)?;

        frames.extend_from_slice(&buf[..n]);

        loop {
            match defmt_decoder::decode(&frames, &table) {
                Ok((frame, consumed)) => {
                    // NOTE(`[]` indexing) all indices in `table` have already been
                    // verified to exist in the `locs` map
                    let loc = locs.as_ref().map(|locs| &locs[&frame.index()]);

                    let (mut file, mut line, mut mod_path) = (None, None, None);
                    if let Some(loc) = loc {
                        let relpath = if let Ok(relpath) = loc.file.strip_prefix(&current_dir) {
                            relpath
                        } else {
                            // not relative; use full path
                            &loc.file
                        };
                        file = Some(relpath.display().to_string());
                        line = Some(loc.line as u32);
                        mod_path = Some(loc.module.clone());
                    }

                    // Forward the defmt frame to our logger.
                    defmt_logger::log_defmt(
                        &frame,
                        file.as_deref(),
                        line,
                        mod_path.as_ref().map(|s| &**s),
                    );

                    let num_frames = frames.len();
                    frames.rotate_left(consumed);
                    frames.truncate(num_frames - consumed);
                }
                Err(defmt_decoder::DecodeError::UnexpectedEof) => break,
                Err(defmt_decoder::DecodeError::Malformed) => {
                    log::error!("failed to decode defmt data: {:x?}", frames);
                    Err(defmt_decoder::DecodeError::Malformed)?
                }
            }
        }
    }
}

// the string reported by the `--version` flag
fn version() -> &'static str {
    // version from Cargo.toml e.g. "0.1.4"
    let mut output = env!("CARGO_PKG_VERSION").to_string();

    output.push_str("\nsupported defmt version: ");
    output.push_str(defmt_decoder::DEFMT_VERSION);

    // leak (!) heap memory to create a `&'static str` value. `String` won't work due to how
    // structopt uses the clap API
    // (this is only called once so it's not that bad)
    Box::leak(Box::<str>::from(output))
}
