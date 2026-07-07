use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    ImportResolver, Name, SchemaEngine, SchemaError, SchemaIdentity, SchemaSource, TrueSchema,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaPackage {
    root: PathBuf,
    crate_name: Name,
    version: String,
}

impl SchemaPackage {
    pub fn new(
        root: impl Into<PathBuf>,
        crate_name: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            root: root.into(),
            crate_name: Name::new(crate_name),
            version: version.into(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn crate_name(&self) -> &Name {
        &self.crate_name
    }

    pub fn schema_directory(&self) -> PathBuf {
        self.root.join("schema")
    }

    pub fn lib_schema_path(&self) -> PathBuf {
        self.schema_directory().join("lib.schema")
    }

    pub fn module_schema_path(&self, module: &Name) -> PathBuf {
        self.schema_directory()
            .join(format!("{}.schema", module.as_str().replace(':', "/")))
    }

    pub fn load_lib(&self) -> Result<SchemaModuleSource, SchemaError> {
        self.load_path(Name::new("lib"), self.lib_schema_path())
    }

    pub fn load_module(&self, module: Name) -> Result<SchemaModuleSource, SchemaError> {
        self.load_path(module.clone(), self.module_schema_path(&module))
    }

    pub fn load_modules(&self) -> Result<Vec<SchemaModuleSource>, SchemaError> {
        self.module_schema_paths()?
            .into_iter()
            .map(|path| {
                let module_name = self.module_name_from_schema_path(&path)?;
                self.load_path(module_name, path)
            })
            .collect()
    }

    pub fn lower_lib(&self, engine: &SchemaEngine) -> Result<TrueSchema, SchemaError> {
        self.load_lib()?.lower(engine)
    }

    pub fn lower_modules(&self, engine: &SchemaEngine) -> Result<Vec<TrueSchema>, SchemaError> {
        self.lower_modules_with_resolver(engine, &ImportResolver::new())
    }

    pub fn lower_modules_with_resolver(
        &self,
        engine: &SchemaEngine,
        resolver: &ImportResolver,
    ) -> Result<Vec<TrueSchema>, SchemaError> {
        let package_resolver = resolver.clone().with_package(self.clone());
        self.load_modules()?
            .iter()
            .map(|source| source.lower_with_resolver(engine, &package_resolver))
            .collect()
    }

    fn module_schema_paths(&self) -> Result<Vec<PathBuf>, SchemaError> {
        let mut paths = Vec::new();
        self.collect_schema_paths(&self.schema_directory(), &mut paths)?;
        paths.sort();
        Ok(paths)
    }

    fn collect_schema_paths(
        &self,
        directory: &Path,
        paths: &mut Vec<PathBuf>,
    ) -> Result<(), SchemaError> {
        let mut entries = fs::read_dir(directory)
            .map_err(|error| SchemaError::Io {
                path: directory.display().to_string(),
                reason: error.to_string(),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| SchemaError::Io {
                path: directory.display().to_string(),
                reason: error.to_string(),
            })?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                self.collect_schema_paths(&path, paths)?;
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("schema") {
                paths.push(path);
            }
        }
        Ok(())
    }

    fn module_name_from_schema_path(&self, path: &Path) -> Result<Name, SchemaError> {
        let schema_directory = self.schema_directory();
        let relative =
            path.strip_prefix(&schema_directory)
                .map_err(|_| SchemaError::MalformedSchemaPath {
                    path: path.display().to_string(),
                })?;
        let mut segments = Vec::new();
        for component in relative.components() {
            let std::path::Component::Normal(segment) = component else {
                return Err(SchemaError::MalformedSchemaPath {
                    path: path.display().to_string(),
                });
            };
            let segment = segment
                .to_str()
                .ok_or_else(|| SchemaError::MalformedSchemaPath {
                    path: path.display().to_string(),
                })?;
            segments.push(segment.to_owned());
        }
        let last = segments
            .last_mut()
            .ok_or_else(|| SchemaError::MalformedSchemaPath {
                path: path.display().to_string(),
            })?;
        let Some(module_file) = last.strip_suffix(".schema") else {
            return Err(SchemaError::MalformedSchemaPath {
                path: path.display().to_string(),
            });
        };
        *last = module_file.to_owned();
        Ok(Name::new(segments.join(":")))
    }

    fn load_path(
        &self,
        module_name: Name,
        path: impl Into<PathBuf>,
    ) -> Result<SchemaModuleSource, SchemaError> {
        let path = path.into();
        let source = fs::read_to_string(&path).map_err(|error| SchemaError::Io {
            path: path.display().to_string(),
            reason: error.to_string(),
        })?;
        Ok(SchemaModuleSource {
            identity: SchemaIdentity::new(
                format!("{}:{}", self.crate_name, module_name),
                self.version.clone(),
            ),
            path,
            source,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaModuleSource {
    identity: SchemaIdentity,
    path: PathBuf,
    source: String,
}

impl SchemaModuleSource {
    pub fn new(
        identity: SchemaIdentity,
        path: impl Into<PathBuf>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            identity,
            path: path.into(),
            source: source.into(),
        }
    }

    pub fn identity(&self) -> &SchemaIdentity {
        &self.identity
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn lower(&self, engine: &SchemaEngine) -> Result<TrueSchema, SchemaError> {
        engine.lower_schema_source(&self.to_schema_source()?, self.identity.clone())
    }

    pub fn lower_with_resolver(
        &self,
        engine: &SchemaEngine,
        resolver: &ImportResolver,
    ) -> Result<TrueSchema, SchemaError> {
        engine.lower_schema_source_with_resolver(
            &self.to_schema_source()?,
            self.identity.clone(),
            resolver,
        )
    }

    pub fn to_schema_source(&self) -> Result<SchemaSource, SchemaError> {
        SchemaSource::from_schema_text(&self.source)
    }
}
