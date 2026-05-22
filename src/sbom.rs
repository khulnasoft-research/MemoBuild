//! SBOM Generation (Software Bill of Materials)
//!
//! This module generates CycloneDX SBOMs for built OCI images.
//! It lists all resolved dependencies, COPY-sourced files, and layer metadata.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomGenerator {
    component_type: String,
    supplier: Option<String>,
}

impl SbomGenerator {
    pub fn new(supplier: Option<String>) -> Self {
        Self {
            component_type: "application".to_string(),
            supplier,
        }
    }

    pub fn generate_sbom(
        &self,
        image_name: &str,
        image_digest: &str,
        layers: &[LayerInfo],
        dependencies: &[Dependency],
        file_components: &[Component],
    ) -> Result<Sbom> {
        let mut components = file_components.to_vec();
        components.extend(dependencies.iter().map(|dep| Component {
            r#type: "library".to_string(),
            name: dep.name.clone(),
            version: dep.version.clone(),
            purl: Some(dep.purl.clone()),
            hash: dep.sha256.clone(),
            licenses: dep.licenses.clone(),
        }));

        let metadata = Metadata {
            timestamp: chrono::Utc::now().to_rfc3339(),
            tools: vec![Tool {
                name: "memobuild".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            }],
            component: Component {
                r#type: "application".to_string(),
                name: image_name.to_string(),
                version: image_digest
                    .get(..12)
                    .unwrap_or(image_digest)
                    .to_string(),
                purl: Some(format!("oci://{}@{}", image_name, image_digest)),
                hash: image_digest.to_string(),
                licenses: vec![],
            },
        };

        Ok(Sbom {
            bom_format: "CycloneDX".to_string(),
            spec_version: "1.5".to_string(),
            serial_number: format!("urn:uuid:{}", uuid::Uuid::new_v4()),
            version: 1,
            metadata,
            components,
            dependencies: dependencies.to_vec(),
            layers: layers.to_vec(),
        })
    }

    pub fn generate_from_context(
        &self,
        image_name: &str,
        image_digest: &str,
        context_dir: &Path,
        dockerfile_path: &Path,
    ) -> Result<Sbom> {
        let dockerfile = fs::read_to_string(dockerfile_path)?;
        let file_components = self.collect_dockerfile_components(context_dir, &dockerfile)?;
        let dependencies = self.collect_dependencies(context_dir)?;
        let layers = self.collect_layers(context_dir)?;
        self.generate_sbom(image_name, image_digest, &layers, &dependencies, &file_components)
    }

    fn collect_dockerfile_components(
        &self,
        context_dir: &Path,
        dockerfile: &str,
    ) -> Result<Vec<Component>> {
        let mut components = Vec::new();

        for line in dockerfile.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("COPY ") || trimmed.starts_with("ADD ") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    let source = parts[1];
                    let source_path = context_dir.join(source);
                    if source_path.exists() {
                        let hash = self.compute_file_hash(&source_path)?;
                        components.push(Component {
                            r#type: "file".to_string(),
                            name: source.to_string(),
                            version: "1.0.0".to_string(),
                            purl: Some(format!("pkg:generic/{}@{}", source, &hash[7..19])),
                            hash,
                            licenses: vec![],
                        });
                    } else {
                        components.push(Component {
                            r#type: "file".to_string(),
                            name: source.to_string(),
                            version: "1.0.0".to_string(),
                            purl: None,
                            hash: "sha256:missing".to_string(),
                            licenses: vec![],
                        });
                    }
                }
            }
        }

        Ok(components)
    }

    fn collect_dependencies(&self, context_dir: &Path) -> Result<Vec<Dependency>> {
        let mut deps = Vec::new();

        let cargo_lock = context_dir.join("Cargo.lock");
        if cargo_lock.exists() {
            let content = fs::read_to_string(&cargo_lock)?;
            let mut current_name = None;
            let mut current_version = None;
            let mut in_package = false;

            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed == "[[package]]" {
                    in_package = true;
                    current_name = None;
                    current_version = None;
                    continue;
                }
                if in_package {
                    if let Some(name) = trimmed.strip_prefix("name = \"") {
                        current_name = Some(name.trim_end_matches('"').to_string());
                    }
                    if let Some(version) = trimmed.strip_prefix("version = \"") {
                        current_version = Some(version.trim_end_matches('"').to_string());
                    }
                    if let Some(name) = &current_name {
                        if let Some(version) = &current_version {
                            let sha256 = format!("sha256:{}", self.compute_hash(format!("{}@{}", name, version).as_bytes())?);
                            deps.push(Dependency {
                                name: name.clone(),
                                version: version.clone(),
                                purl: format!("pkg:cargo/{}/{}", name, version),
                                sha256,
                                licenses: vec![],
                            });
                            in_package = false;
                        }
                    }
                }
            }
        }

        let package_lock = context_dir.join("package-lock.json");
        if package_lock.exists() {
            let lockfile: Value = serde_json::from_str(&fs::read_to_string(&package_lock)?)?;
            if let Some(root_deps) = lockfile.get("dependencies") {
                self.collect_npm_deps(root_deps, "", &mut deps)?;
            }
        }

        Ok(deps)
    }

    fn collect_npm_deps(&self, node: &Value, parent: &str, deps: &mut Vec<Dependency>) -> Result<()> {
        if let Value::Object(map) = node {
            for (name, details) in map {
                if let Some(version) = details.get("version").and_then(|v| v.as_str()) {
                    let sha256 = format!("sha256:{}", self.compute_hash(format!("{}@{}", name, version).as_bytes())?);
                    deps.push(Dependency {
                        name: if parent.is_empty() {
                            name.clone()
                        } else {
                            format!("{}/{}", parent, name)
                        },
                        version: version.to_string(),
                        purl: format!("pkg:npm/{}/{}", parent.trim_start_matches('/'), name),
                        sha256,
                        licenses: vec![],
                    });
                }
                if let Some(child) = details.get("dependencies") {
                    let next_parent = if parent.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", parent, name)
                    };
                    self.collect_npm_deps(child, &next_parent, deps)?;
                }
            }
        }
        Ok(())
    }

    fn collect_layers(&self, context_dir: &Path) -> Result<Vec<LayerInfo>> {
        let mut layers = Vec::new();
        let oci_blobs = context_dir.join("blobs").join("sha256");
        if oci_blobs.exists() {
            for entry in fs::read_dir(&oci_blobs)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    let digest = format!("sha256:{}", entry.file_name().to_string_lossy());
                    let size = fs::metadata(&path)?.len();
                    layers.push(LayerInfo {
                        digest,
                        size,
                        media_type: "application/vnd.oci.image.layer.v1.tar+gzip".to_string(),
                    });
                }
            }
        }
        Ok(layers)
    }

    fn compute_file_hash(&self, path: &Path) -> Result<String> {
        let bytes = fs::read(path)?;
        Ok(format!("sha256:{}", self.compute_hash(&bytes)?))
    }

    fn compute_hash(&self, bytes: &[u8]) -> Result<String> {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Ok(hex::encode(hasher.finalize()))
    }

    pub fn to_json(&self, sbom: &Sbom) -> Result<String> {
        serde_json::to_string_pretty(sbom)
            .map_err(|e| anyhow::anyhow!("Failed to serialize SBOM: {}", e))
    }

    pub fn to_xml(&self, sbom: &Sbom) -> Result<String> {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&format!("<bom serialNumber=\"{}\" version=\"1\" xmlns=\"http://cyclonedx.org/schema/bom/1.5\">\n", sbom.serial_number));
        xml.push_str("  <metadata>\n");
        xml.push_str(&format!("    <timestamp>{}</timestamp>\n", sbom.metadata.timestamp));
        xml.push_str("    <tools>\n");
        for tool in &sbom.metadata.tools {
            xml.push_str("      <tool>\n");
            xml.push_str(&format!("        <name>{}</name>\n", tool.name));
            xml.push_str(&format!("        <version>{}</version>\n", tool.version));
            xml.push_str("      </tool>\n");
        }
        xml.push_str("    </tools>\n");
        xml.push_str(&format!("    <component>\n      <name>{}</name>\n      <version>{}</version>\n    </component>\n", sbom.metadata.component.name, sbom.metadata.component.version));
        xml.push_str("  </metadata>\n");
        xml.push_str("</bom>\n");
        Ok(xml)
    }

    pub fn save_sbom(&self, sbom: &Sbom, path: &Path, format: &OutputFormat) -> Result<()> {
        let output = match format {
            OutputFormat::Xml => self.to_xml(sbom)?,
            OutputFormat::Json => self.to_json(sbom)?,
        };
        fs::write(path, output)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sbom {
    #[serde(rename = "bomFormat")]
    bom_format: String,
    #[serde(rename = "specVersion")]
    spec_version: String,
    #[serde(rename = "serialNumber")]
    serial_number: String,
    version: i32,
    metadata: Metadata,
    components: Vec<Component>,
    dependencies: Vec<Dependency>,
    layers: Vec<LayerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    timestamp: String,
    tools: Vec<Tool>,
    component: Component,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    name: String,
    version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    #[serde(rename = "type")]
    r#type: String,
    name: String,
    version: String,
    purl: Option<String>,
    #[serde(rename = "hash")]
    hash: String,
    licenses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub purl: String,
    pub sha256: String,
    pub licenses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerInfo {
    pub digest: String,
    pub size: u64,
    pub media_type: String,
}

#[derive(Debug, Clone)]
pub enum OutputFormat {
    Json,
    Xml,
}

/// CLI for SBOM generation
pub mod cli {
    use super::*;
    use std::path::Path;

    pub fn generate_cmd(
        image: &str,
        context_dir: &Path,
        dockerfile: &Path,
        output: &str,
        format: &str,
    ) -> Result<()> {
        let generator = SbomGenerator::new(Some("NRELabs".to_string()));
        let sbom = generator.generate_from_context(image, image, context_dir, dockerfile)?;
        let parsed_format = match format {
            "xml" => OutputFormat::Xml,
            _ => OutputFormat::Json,
        };
        if output == "-" {
            let serialized = match parsed_format {
                OutputFormat::Json => generator.to_json(&sbom)?,
                OutputFormat::Xml => generator.to_xml(&sbom)?,
            };
            println!("{}", serialized);
        } else {
            generator.save_sbom(&sbom, Path::new(output), &parsed_format)?;
            println!("SBOM written to {}", output);
        }
        Ok(())
    }
}
