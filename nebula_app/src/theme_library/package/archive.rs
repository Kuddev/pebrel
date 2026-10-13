use super::*;
use crate::theme_library::{MAX_DOCUMENT_BYTES, ThemeDocument, ThemeLibraryStore};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub(crate) struct CheckedPackage {
    archive: ZipArchive<super::envelope::PackageFile>,
    pub manifest: Manifest,
    pub document: ThemeDocument,
}

impl CheckedPackage {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let (reader, metadata_only, entries) = super::envelope::reader(file)?;
        let mut archive = ZipArchive::new(reader)?;
        metadata_only.store(false, std::sync::atomic::Ordering::Relaxed);
        require(archive.len() == entries, "duplicate ZIP entry")?;
        let mut names = BTreeSet::new();
        let mut unpacked = 0u64;
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            validate_path(entry.name())?;
            require(!entry.is_dir(), "directory entries are not part of the package format")?;
            require(!entry.encrypted(), "encrypted ZIP entries are not supported")?;
            require(
                entry
                    .unix_mode()
                    .is_none_or(|mode| mode & 0o170000 == 0 || mode & 0o170000 == 0o100000),
                "nonregular ZIP entry",
            )?;
            require(
                matches!(
                    entry.compression(),
                    CompressionMethod::Stored | CompressionMethod::Deflated
                ),
                "unsupported ZIP compression",
            )?;
            require(names.insert(entry.name().to_ascii_lowercase()), "duplicate ZIP entry")?;
            unpacked = unpacked
                .checked_add(entry.size())
                .ok_or_else(|| PackageError("size overflow".into()))?;
            require(unpacked <= MAX_UNPACKED_BYTES, "unpacked package exceeds 64 MiB")?;
        }
        let manifest: Manifest =
            serde_json::from_slice(&read_small(&mut archive, MANIFEST, MAX_MANIFEST_BYTES)?)?;
        manifest.validate()?;
        let document =
            ThemeDocument::from_json_bytes(&read_small(&mut archive, THEME, MAX_DOCUMENT_BYTES)?)
                .map_err(|error| PackageError(error.to_string()))?;
        require(document.name() == manifest.name, "manifest and theme names differ")?;
        let mut expected = BTreeSet::from([MANIFEST.to_owned(), THEME.to_owned()]);
        for resource in &manifest.resources {
            expected.insert(resource.path.to_ascii_lowercase());
            verify_resource(&mut archive, resource, &mut std::io::sink())?;
        }
        require(names == expected, "ZIP contains undeclared or missing entries")?;
        if let Some(path) = document
            .definition()
            .map_err(|e| PackageError(e.to_string()))?
            .effects
            .background_image
            .filter(|s| !s.is_empty())
        {
            validate_path(&path)?;
            require(
                manifest.resources.iter().any(|r| {
                    r.path == path
                        && matches!(
                            r.kind,
                            ResourceKind::Image | ResourceKind::AnimatedImage | ResourceKind::Video
                        )
                }),
                "background must refer to a declared media resource",
            )?;
        }
        Ok(Self { archive, manifest, document })
    }

    /// Installation only publishes a new library document after all assets are
    /// verified on disk. It never activates the theme or overwrites a theme ID.
    pub(crate) fn install(mut self, store: &ThemeLibraryStore) -> Result<ThemeDocument> {
        require(
            self.manifest.resources.iter().all(|resource| {
                matches!(resource.kind, ResourceKind::Image | ResourceKind::Preview)
            }),
            "package requires animated media or shader capabilities not available in this build",
        )?;
        let root = store.root().join("packages");
        fs::create_dir_all(&root)?;
        require(
            !fs::symlink_metadata(&root)?.file_type().is_symlink(),
            "package directory must not be a symbolic link",
        )?;
        let root = fs::canonicalize(root)?;
        let stage = tempfile::Builder::new().prefix("theme-").tempdir_in(&root)?;
        for resource in &self.manifest.resources {
            let output = stage.path().join(&resource.path);
            fs::create_dir_all(output.parent().unwrap())?;
            let mut file = File::options().write(true).create_new(true).open(&output)?;
            verify_resource(&mut self.archive, resource, &mut file)?;
            file.sync_all()?;
        }
        fs::write(stage.path().join(MANIFEST), serde_json::to_vec_pretty(&self.manifest)?)?;
        let mut value = self.document.to_value();
        let image = self
            .document
            .definition()
            .map_err(|e| PackageError(e.to_string()))?
            .effects
            .background_image;
        if let Some(path) = image.filter(|path| !path.is_empty()) {
            let absolute = stage.path().join(path);
            value["effects"]["background_image"] = serde_json::Value::String(
                absolute
                    .to_str()
                    .ok_or_else(|| PackageError("package path is not UTF-8".into()))?
                    .to_owned(),
            );
        }
        let metadata = value
            .as_object_mut()
            .unwrap()
            .entry("metadata")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .unwrap();
        metadata.insert(
            "package".into(),
            serde_json::json!({
                "version": self.manifest.version, "author": self.manifest.author,
                "license": self.manifest.license,
                "resource_directory": stage.path().file_name().unwrap().to_string_lossy(),
            }),
        );
        let resolved = ThemeDocument::from_value(value).map_err(|e| PackageError(e.to_string()))?;
        // The directory already has its final random name. TempDir rolls it
        // back if persistence fails; a completed import transfers ownership.
        let stored = store.import(&resolved, None).map_err(|e| PackageError(e.to_string()))?;
        let _path = stage.keep();
        Ok(stored)
    }
}

fn read_small(
    archive: &mut ZipArchive<super::envelope::PackageFile>,
    path: &str,
    limit: usize,
) -> Result<Vec<u8>> {
    let mut entry = archive.by_name(path)?;
    require(entry.size() <= limit as u64, format!("{path} exceeds metadata limit"))?;
    let mut bytes = Vec::new();
    entry.by_ref().take(limit as u64 + 1).read_to_end(&mut bytes)?;
    require(
        bytes.len() <= limit && bytes.len() as u64 == entry.size(),
        "metadata stream size differs from directory or exceeds limit",
    )?;
    Ok(bytes)
}

fn stream<R: Read, W: Write>(reader: &mut R, writer: &mut W, limit: u64) -> Result<(u64, String)> {
    let mut buffer = [0u8; 64 * 1024];
    let mut bytes = 0u64;
    let mut digest = Sha256::new();
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(read as u64)
            .ok_or_else(|| PackageError("stream size overflow".into()))?;
        require(bytes <= limit, "resource stream exceeds declared size")?;
        digest.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
    }
    use std::fmt::Write as _;
    let mut hash = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(&mut hash, "{byte:02x}").unwrap();
    }
    Ok((bytes, hash))
}

fn verify_resource<W: Write>(
    archive: &mut ZipArchive<super::envelope::PackageFile>,
    resource: &Resource,
    output: &mut W,
) -> Result<()> {
    let mut entry = archive.by_name(&resource.path)?;
    require(entry.size() == resource.bytes, "ZIP size differs from manifest")?;
    let mut header = vec![0u8; resource.bytes.min(64) as usize];
    entry.read_exact(&mut header)?;
    if matches!(resource.kind, ResourceKind::Image | ResourceKind::Preview) {
        require(
            matches!(
                image::guess_format(&header),
                Ok(image::ImageFormat::Png
                    | image::ImageFormat::Jpeg
                    | image::ImageFormat::Bmp
                    | image::ImageFormat::WebP)
            ),
            "image resource has an invalid media signature",
        )?;
    }
    let mut source = std::io::Cursor::new(header).chain(entry);
    let (bytes, hash) = stream(&mut source, output, resource.bytes)?;
    require(
        bytes == resource.bytes && hash == resource.sha256,
        "resource integrity verification failed",
    )
}

pub(crate) fn export_package(
    document: &ThemeDocument,
    author: Author,
    version: String,
    license: String,
    output: &Path,
    preview: Option<&Path>,
) -> Result<()> {
    let mut definition = document.definition().map_err(|e| PackageError(e.to_string()))?;
    let mut resources = Vec::new();
    let mut inputs = BTreeMap::<String, PathBuf>::new();
    if let Some(path) =
        definition.effects.background_image.as_deref().filter(|path| !path.is_empty())
    {
        let path = PathBuf::from(path);
        let extension =
            path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        require(
            matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "webp"),
            "this exporter supports static background image files",
        )?;
        let entry = format!("assets/background.{extension}");
        let mut file = File::open(&path)?;
        require(file.metadata()?.is_file(), "background must be a regular file")?;
        let (bytes, sha256) = stream(
            &mut file,
            &mut std::io::sink(),
            MAX_UNPACKED_BYTES - MAX_DOCUMENT_BYTES as u64 - MAX_MANIFEST_BYTES as u64,
        )?;
        resources.push(Resource { path: entry.clone(), kind: ResourceKind::Image, bytes, sha256 });
        inputs.insert(entry.clone(), path);
        definition.effects.background_image = Some(entry);
    }
    if let Some(path) = preview {
        let extension =
            path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        require(
            matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "webp"),
            "preview must be a supported image file",
        )?;
        let entry = format!("assets/preview.{extension}");
        let mut file = File::open(path)?;
        require(file.metadata()?.is_file(), "preview must be a regular file")?;
        let (bytes, sha256) = stream(&mut file, &mut std::io::sink(), MAX_UNPACKED_BYTES)?;
        resources.push(Resource {
            path: entry.clone(),
            kind: ResourceKind::Preview,
            bytes,
            sha256,
        });
        inputs.insert(entry, path.to_owned());
    }
    let portable =
        document.with_definition(&definition).map_err(|e| PackageError(e.to_string()))?;
    let mut value = portable.to_value();
    // Machine-local installation receipts are not portable package metadata.
    if let Some(metadata) = value.get_mut("metadata").and_then(serde_json::Value::as_object_mut) {
        metadata.remove("package");
    }
    let portable = ThemeDocument::from_value(value).map_err(|e| PackageError(e.to_string()))?;
    let manifest = Manifest {
        package_version: 1,
        name: document.name().to_owned(),
        author,
        version,
        license,
        theme: THEME.into(),
        resources,
    };
    manifest.validate()?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    require(manifest_bytes.len() <= MAX_MANIFEST_BYTES, "manifest exceeds 64 KiB")?;
    let theme_bytes = portable.to_json_bytes().map_err(|e| PackageError(e.to_string()))?;
    let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut writer = ZipWriter::new(temporary.reopen()?);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);
    writer.start_file(MANIFEST, options)?;
    writer.write_all(&manifest_bytes)?;
    writer.start_file(THEME, options)?;
    writer.write_all(&theme_bytes)?;
    for resource in &manifest.resources {
        writer.start_file(&resource.path, options)?;
        let mut source = File::open(&inputs[&resource.path])?;
        let (bytes, hash) = stream(&mut source, &mut writer, resource.bytes)?;
        require(
            bytes == resource.bytes && hash == resource.sha256,
            "background changed during export",
        )?;
    }
    let completed = writer.finish()?;
    require(completed.metadata()?.len() <= MAX_ARCHIVE_BYTES, "theme ZIP exceeds 48 MiB")?;
    completed.sync_all()?;
    drop(completed);
    // Verify the finished artifact before atomically replacing the destination.
    CheckedPackage::open(temporary.path())?;
    crate::atomic_file::replace(temporary.path(), output)?;
    Ok(())
}
