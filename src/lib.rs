//! A small crate for hot-loading GLSL as SPIR-V!
//!
//! See the `watch` function.

use notify::{self, Watcher};
use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use thiserror::Error;

/// Watches one or more paths for changes to GLSL shader files.
///
/// See the `watch` or `watch_paths` constructor functions.
pub struct Watch {
    event_rx: mpsc::Receiver<notify::Result<notify::Event>>,
    pending_paths: RefCell<Vec<PathBuf>>,
    _watcher: notify::RecommendedWatcher,
    _watched_paths: Vec<PathBuf>,
}

/// Errors that might occur while creating a `Watch` instance.
#[derive(Debug, Error)]
#[error("failed to setup a notify watcher: {err}")]
pub struct CreationError {
    #[from]
    err: notify::Error,
}

/// Errors that might occur while waiting for the next library instance.
#[derive(Debug, Error)]
pub enum NextPathError {
    #[error("the channel used to receive file system events was closed")]
    ChannelClosed,
    #[error("a notify event signalled an error: {err}")]
    Notify {
        #[from]
        err: notify::Error,
    },
}

/// Errors that might occur while waiting for the next file system event.
#[derive(Debug, Error)]
pub enum AwaitEventError {
    #[error("the channel used to receive file system events was closed")]
    ChannelClosed,
    #[error("a notify event signalled an error: {err}")]
    Notify {
        #[from]
        err: notify::Error,
    },
}

/// Errors that might occur while attempting to compile a glsl file to a spir-v file.
#[derive(Debug, Error)]
pub enum CompileError {
    #[error("an I/O error occurred: {err}")]
    Io {
        #[from]
        err: std::io::Error,
    },
    #[error("an error occurred during `glsl_to_spirv::compile`: {err}")]
    GlslToSpirv { err: String },
}

/// The list of extensions that are considered valid shader extensions.
///
/// There are no real official extensions for GLSL files or even an official GLSL file format, but
/// apparently Khronos' reference GLSL compiler/validator uses these.
///
/// This is a subset from which we can infer the shader type (necessary for compiling the shader
/// with `glsl-to-spirv`).
pub const GLSL_EXTENSIONS: &[&str] = &["vert", "frag", "comp", "vs", "fs", "cs"];

impl Watch {
    /// Block the current thread until some filesystem event has been received from notify.
    ///
    /// This is useful when running the hotloading process on a separate thread.
    pub fn await_event(&self) -> Result<(), AwaitEventError> {
        let res = match self.event_rx.recv() {
            Ok(res) => res,
            _ => return Err(AwaitEventError::ChannelClosed),
        };
        let event = res?;
        let paths = shaders_related_to_event(&event);
        self.pending_paths.borrow_mut().extend(paths);
        Ok(())
    }

    /// Checks for a new filesystem event.
    ///
    /// If the event relates to a shader file, the path to that event is returned.
    ///
    /// If the event relates to multiple shader files, the remaining files are buffered until the
    /// next call to `next` or `try_next`.
    ///
    /// Returns an `Err` if the channel was closed or if one of the notify `Watcher`s sent us an
    /// error.
    pub fn try_next_path(&self) -> Result<Option<PathBuf>, NextPathError> {
        let mut pending_paths = self.pending_paths.borrow_mut();
        loop {
            if !pending_paths.is_empty() {
                return Ok(Some(pending_paths.remove(0)));
            }
            match self.event_rx.try_recv() {
                Err(mpsc::TryRecvError::Disconnected) => return Err(NextPathError::ChannelClosed),
                Err(mpsc::TryRecvError::Empty) => (),
                Ok(res) => {
                    let event = res?;
                    pending_paths.extend(shaders_related_to_event(&event));
                    continue;
                }
            }
            return Ok(None);
        }
    }

    /// Returns all unique paths that have been changed at least once since the last call to
    /// `paths_touched` or `compile_touched`.
    ///
    /// This uses `try_next_path` internally to collect all pending paths into a set containing
    /// each unique path only once.
    pub fn paths_touched(&self) -> Result<HashSet<PathBuf>, NextPathError> {
        let mut paths = HashSet::new();
        loop {
            match self.try_next_path() {
                Err(err) => return Err(err),
                Ok(None) => break,
                Ok(Some(path)) => {
                    paths.insert(path);
                }
            }
        }
        Ok(paths)
    }

    /// Produce an iterator that compiles each touched shader file to SPIR-V.
    ///
    /// Compilation of each file only begins on the produced iterator's `next` call.
    pub fn compile_touched(
        &self,
    ) -> Result<impl Iterator<Item = (PathBuf, Result<Vec<u8>, CompileError>)>, NextPathError> {
        let paths = self.paths_touched()?;
        let iter = paths.into_iter().map(|path| {
            let result = compile(&path);
            (path, result)
        });
        Ok(iter)
    }
}

/// Watch the give file or directory of files.
pub fn watch<P>(path: P) -> Result<Watch, CreationError>
where
    P: AsRef<Path>,
{
    watch_paths(Some(path))
}

/// Watch each of the specified paths for events.
pub fn watch_paths<I>(paths: I) -> Result<Watch, CreationError>
where
    I: IntoIterator,
    I::Item: AsRef<Path>,
{
    // Channel for sending events back to the main thread.
    let (tx, event_rx) = mpsc::channel();

    // Create a watcher for each path.
    let mut watched_paths = vec![];
    let mut watcher = notify::RecommendedWatcher::new_immediate(move |res| {
        tx.send(res).ok();
    })?;
    for path in paths {
        let path = path.as_ref().to_path_buf();
        if path.is_dir() {
            watcher.watch(&path, notify::RecursiveMode::Recursive)?;
        } else {
            watcher.watch(&path, notify::RecursiveMode::NonRecursive)?;
        }
        watched_paths.push(path);
    }

    let pending_paths = RefCell::new(vec![]);
    Ok(Watch {
        event_rx,
        pending_paths,
        _watcher: watcher,
        _watched_paths: watched_paths,
    })
}

/// Checks whether or not the event relates to some shader file, and if so, returns the path to
/// that shader file.
fn shaders_related_to_event<'a>(event: &'a notify::Event) -> impl 'a + Iterator<Item = PathBuf> {
    event.paths.iter().filter_map(|p| {
        if path_is_shader_file(p) {
            Some(p.to_path_buf())
        } else {
            None
        }
    })
}

/// Whether or not the given path is a shader file.
///
/// This is used when watching directories to distinguish between files that are shaders and those
/// that are not.
fn path_is_shader_file(path: &Path) -> bool {
    if path.is_file() {
        let path_ext = match path.extension().and_then(|s| s.to_str()) {
            None => return false,
            Some(ext) => ext,
        };
        for ext in GLSL_EXTENSIONS {
            if &path_ext == ext {
                return true;
            }
        }
    }
    false
}

/// Compile the GLSL file at the given path to SPIR-V.
///
/// The shader type is inferred from the path extension.
///
/// Returns a `Vec<u8>` containing raw SPIR-V bytes.
pub fn compile(glsl_path: &Path) -> Result<Vec<u8>, CompileError> {
    // Infer the shader type.
    let shader_ty = glsl_path
        .extension()
        .and_then(|s| s.to_str())
        .and_then(extension_to_shader_ty)
        .expect("");

    // Compile to spirv.
    let glsl_string = std::fs::read_to_string(glsl_path)?;
    let spirv_file = glsl_to_spirv::compile(&glsl_string, shader_ty)
        .map_err(|err| CompileError::GlslToSpirv { err })?;

    // Read generated file to bytes.
    let mut buf_reader = std::io::BufReader::new(spirv_file);
    let mut spirv_bytes = vec![];
    buf_reader.read_to_end(&mut spirv_bytes)?;
    Ok(spirv_bytes)
}

/// Convert the given file extension to a shader type for `glsl_to_spirv` compilation.
fn extension_to_shader_ty(ext: &str) -> Option<glsl_to_spirv::ShaderType> {
    let ty = match ext {
        "vert" => glsl_to_spirv::ShaderType::Vertex,
        "frag" => glsl_to_spirv::ShaderType::Fragment,
        "comp" => glsl_to_spirv::ShaderType::Compute,
        "vs" => glsl_to_spirv::ShaderType::Vertex,
        "fs" => glsl_to_spirv::ShaderType::Fragment,
        "cs" => glsl_to_spirv::ShaderType::Compute,
        _ => return None,
    };
    Some(ty)
}
