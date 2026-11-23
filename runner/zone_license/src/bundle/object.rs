use super::architecture::Architecture;
use super::file::File;
use super::version::BundleVersion;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Object {
    version: BundleVersion,
    architecture: Architecture,
    file: File,
}

impl Object {
    pub fn new(version: BundleVersion, architecture: Architecture, file: File) -> Self {
        Self {
            version,
            architecture,
            file,
        }
    }

    pub fn files(version: BundleVersion, architecture: Architecture) -> [Self; 3] {
        File::ALL.map(|file| Self::new(version, architecture, file))
    }

    pub fn marker(version: BundleVersion, architecture: Architecture) -> Self {
        Self::new(version, architecture, File::Marker)
    }

    pub fn version(&self) -> BundleVersion {
        self.version
    }

    pub fn architecture(&self) -> Architecture {
        self.architecture
    }

    pub fn file(&self) -> File {
        self.file
    }

    pub fn name(&self) -> String {
        format!(
            "zone-bundle-{}-{}{}",
            self.version,
            self.architecture,
            self.file.extension()
        )
    }

    pub fn key(&self, prefix: &str) -> String {
        format!("{prefix}/{}/{}", self.version, self.name())
    }
}
