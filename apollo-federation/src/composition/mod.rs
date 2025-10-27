mod satisfiability;

use std::vec;

pub use crate::composition::satisfiability::validate_satisfiability;
use crate::error::CompositionError;
pub use crate::schema::schema_upgrader::upgrade_subgraphs_if_necessary;
use crate::subgraph::typestate::Expanded;
use crate::subgraph::typestate::Initial;
use crate::subgraph::typestate::Subgraph;
use crate::subgraph::typestate::Upgraded;
use crate::subgraph::typestate::Validated;
pub use crate::supergraph::Merged;
pub use crate::supergraph::Satisfiable;
pub use crate::supergraph::Supergraph;

/// Composition options that control the behavior of the composition process
#[derive(Debug, Clone, Default)]
pub struct CompositionOptions {
    /// Flag to toggle if satisfiability should be performed during composition
    /// Defaults to `true`
    pub run_satisfiability: bool,
    /// Maximum allowable number of outstanding subgraph paths to validate
    pub max_validation_subgraph_paths: Option<usize>,
}

impl CompositionOptions {
    /// Create new composition options with default values
    pub fn new() -> Self {
        Self {
            run_satisfiability: true,
            max_validation_subgraph_paths: None,
        }
    }
    
    /// Set whether to run satisfiability validation
    pub fn with_satisfiability(mut self, run_satisfiability: bool) -> Self {
        self.run_satisfiability = run_satisfiability;
        self
    }
    
    /// Set the maximum validation subgraph paths
    pub fn with_max_validation_paths(mut self, max_paths: usize) -> Self {
        self.max_validation_subgraph_paths = Some(max_paths);
        self
    }
}

// Re-export CompositionHint from supergraph module
pub use crate::supergraph::CompositionHint;

/// Result of the composition process
#[derive(Debug)]
pub enum CompositionResult {
    /// Successful composition
    Success {
        /// The composed supergraph
        supergraph: Supergraph<Satisfiable>,
        /// Hints generated during composition
        hints: Vec<CompositionHint>,
    },
    /// Failed composition
    Failure {
        /// Errors that occurred during composition
        errors: Vec<CompositionError>,
    },
}

/// Compose subgraphs into a supergraph with default options
pub fn compose(
    subgraphs: Vec<Subgraph<Initial>>,
) -> Result<Supergraph<Satisfiable>, Vec<CompositionError>> {
    match compose_with_options(subgraphs, CompositionOptions::new()) {
        CompositionResult::Success { supergraph, .. } => Ok(supergraph),
        CompositionResult::Failure { errors } => Err(errors),
    }
}

/// Compose subgraphs into a supergraph with custom options and return detailed results
pub fn compose_with_options(
    subgraphs: Vec<Subgraph<Initial>>,
    options: CompositionOptions,
) -> CompositionResult {
    let mut all_hints = Vec::new();
    
    // Expand subgraphs
    let expanded_subgraphs = match expand_subgraphs(subgraphs) {
        Ok(subgraphs) => subgraphs,
        Err(errors) => return CompositionResult::Failure { errors },
    };
    
    // Upgrade subgraphs if necessary
    let upgraded_subgraphs = match upgrade_subgraphs_if_necessary(expanded_subgraphs) {
        Ok(subgraphs) => subgraphs,
        Err(errors) => return CompositionResult::Failure { errors },
    };
    
    // Validate subgraphs
    let validated_subgraphs = match validate_subgraphs(upgraded_subgraphs) {
        Ok(subgraphs) => subgraphs,
        Err(errors) => return CompositionResult::Failure { errors },
    };

    // Pre-merge validations
    if let Err(errors) = pre_merge_validations(&validated_subgraphs) {
        return CompositionResult::Failure { errors };
    }
    
    // Merge subgraphs
    let (supergraph, merge_hints) = match merge_subgraphs_with_hints(validated_subgraphs) {
        Ok(result) => result,
        Err(errors) => return CompositionResult::Failure { errors },
    };
    all_hints.extend(merge_hints);
    
    // Post-merge validations
    if let Err(errors) = post_merge_validations(&supergraph) {
        return CompositionResult::Failure { errors };
    }
    
    // Satisfiability validation (optional)
    if options.run_satisfiability {
        match validate_satisfiability_with_hints(supergraph) {
            Ok((satisfiable_supergraph, satisfiability_hints)) => {
                all_hints.extend(satisfiability_hints);
                CompositionResult::Success {
                    supergraph: satisfiable_supergraph,
                    hints: all_hints,
                }
            }
            Err(errors) => CompositionResult::Failure { errors },
        }
    } else {
        // Skip satisfiability validation - assume satisfiable
        CompositionResult::Success {
            supergraph: supergraph.assume_satisfiable(),
            hints: all_hints,
        }
    }
}

/// Apollo Federation allow subgraphs to specify partial schemas (i.e. "import" directives through
/// `@link`). This function will update subgraph schemas with all missing federation definitions.
pub fn expand_subgraphs(
    subgraphs: Vec<Subgraph<Initial>>,
) -> Result<Vec<Subgraph<Expanded>>, Vec<CompositionError>> {
    let mut errors: Vec<CompositionError> = vec![];
    let expanded: Vec<Subgraph<Expanded>> = subgraphs
        .into_iter()
        .map(|s| s.expand_links())
        .filter_map(|r| r.map_err(|e| errors.push(e.into())).ok())
        .collect();
    if errors.is_empty() {
        Ok(expanded)
    } else {
        Err(errors)
    }
}

/// Validate subgraph schemas to ensure they satisfy Apollo Federation requirements (e.g. whether
/// `@key` specifies valid `FieldSet`s etc).
pub fn validate_subgraphs(
    subgraphs: Vec<Subgraph<Upgraded>>,
) -> Result<Vec<Subgraph<Validated>>, Vec<CompositionError>> {
    let mut errors: Vec<CompositionError> = vec![];
    let validated: Vec<Subgraph<Validated>> = subgraphs
        .into_iter()
        .map(|s| s.validate())
        .filter_map(|r| r.map_err(|e| errors.push(e.into())).ok())
        .collect();
    if errors.is_empty() {
        Ok(validated)
    } else {
        Err(errors)
    }
}

/// Perform validations that require information about all available subgraphs.
pub fn pre_merge_validations(
    subgraphs: &[Subgraph<Validated>],
) -> Result<(), Vec<CompositionError>> {
    let mut errors = Vec::new();
    
    // Validate subgraph names are unique
    let mut seen_names = std::collections::HashSet::new();
    for subgraph in subgraphs {
        if !seen_names.insert(&subgraph.name) {
            errors.push(CompositionError::InvalidGraphQL {
                message: format!("Duplicate subgraph name: '{}'", subgraph.name),
            });
        }
    }
    
    // Validate that at least one subgraph has a Query type
    let has_query = subgraphs.iter().any(|subgraph| {
        subgraph.schema().schema().schema_definition.query.is_some()
    });
    
    if !has_query {
        errors.push(CompositionError::InvalidGraphQL {
            message: "No queries found in any subgraph: a supergraph must have a query root type.".to_string(),
        });
    }
    
    // Validate federation schema usage across subgraphs
     if !subgraphs.iter().all(|s| s.validated_schema().is_fed_2()) {
        errors.push(CompositionError::InvalidGraphQL {
            message: "Merging should only be applied to federation 2 subgraphs".to_string(),
        });
    }
    
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Performs the actual merging of all validated subgraphs to a supergraph.
/// The validated subgraphs must be convertible to valid federation subgraphs.
pub fn merge_subgraphs(
    subgraphs: Vec<Subgraph<Validated>>,
) -> Result<Supergraph<Merged>, Vec<CompositionError>> {
    let (supergraph, _hints) = merge_subgraphs_with_hints(subgraphs)?;
    Ok(supergraph)
}

/// Performs the actual merging of all validated subgraphs to a supergraph, collecting hints.
fn merge_subgraphs_with_hints(
    subgraphs: Vec<Subgraph<Validated>>,
) -> Result<(Supergraph<Merged>, Vec<CompositionHint>), Vec<CompositionError>> {
    use crate::merge::merge_federation_subgraphs;
    use crate::ValidFederationSubgraphs;
    use crate::ValidFederationSubgraph;
    
    // Convert Subgraph<Validated> to ValidFederationSubgraphs
    let mut federation_subgraphs = ValidFederationSubgraphs::new();
    
    for subgraph in subgraphs {
        let valid_subgraph = ValidFederationSubgraph {
            name: subgraph.name.clone(),
            url: subgraph.url.clone(),
            schema: subgraph.validated_schema().clone(),
        };
        
        federation_subgraphs.add(valid_subgraph)
            .map_err(|e| vec![CompositionError::SubgraphError {
                subgraph: subgraph.name.clone(),
                error: e,
            }])?;
    }
    
    // Perform the actual merging using the existing merge logic
    let merge_result = merge_federation_subgraphs(federation_subgraphs)
        .map_err(|failure| {
            failure.errors.into_iter()
                .map(|error| CompositionError::InvalidGraphQL { message: error })
                .collect::<Vec<_>>()
        })?;
    
    // Convert merge warnings to composition hints
    let hints: Vec<CompositionHint> = merge_result.composition_hints.into_iter()
        .map(|warning| CompositionHint {
            code: "MERGE_WARNING".to_string(),
            message: warning,
        })
        .collect();
    
    // Convert the merge result to Supergraph<Merged>
    Ok((Supergraph::<Merged>::new(merge_result.schema), hints))
}

pub fn post_merge_validations(
    supergraph: &Supergraph<Merged>,
) -> Result<(), Vec<CompositionError>> {
    let mut errors = Vec::new();
    
    let schema = supergraph.schema();
    
    // Validate that the merged supergraph has a Query root type
    if schema.schema_definition.query.is_none() {
        errors.push(CompositionError::InvalidGraphQL {
            message: "Merged supergraph must have a query root type".to_string(),
        });
    }
    
    // Validate interface implementations are complete
    // This mirrors the TypeScript postMergeValidations logic for interface field validation
    validate_interface_implementations(&mut errors, schema);
    
    // Note: The schema is already validated since it's a Valid<Schema>
    // The TypeScript version also validates @requires directives against the supergraph,
    // but that requires access to the original subgraphs which we don't have here(or I have not figured out yet).
    // This validation could ideally be moved to pre_merge_validations or handled
    // during the merge process itself.
    
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates satisfiability and collects hints
fn validate_satisfiability_with_hints(
    supergraph: Supergraph<Merged>,
) -> Result<(Supergraph<Satisfiable>, Vec<CompositionHint>), Vec<CompositionError>> {
    // For now, we don't have a way to collect hints from satisfiability validation
    // This would require modifying the validate_satisfiability function to return hints
    // TODO: Enhance validate_satisfiability to return hints along with the result
    
    match validate_satisfiability(supergraph) {
        Ok(satisfiable_supergraph) => {
            // No hints available from current satisfiability implementation
            Ok((satisfiable_supergraph, Vec::new()))
        }
        Err(errors) => Err(errors),
    }
}

/// Validates that object/interface types properly implement all fields from their interfaces
fn validate_interface_implementations(
    errors: &mut Vec<CompositionError>,
    schema: &apollo_compiler::Schema,
) {
    use apollo_compiler::schema::ExtendedType;
    
    for type_def in schema.types.values() {
        let (type_name, implements_interfaces, type_fields) = match type_def {
            ExtendedType::Object(obj_type) => (
                &obj_type.name,
                &obj_type.implements_interfaces,
                &obj_type.fields,
            ),
            ExtendedType::Interface(intf_type) => (
                &intf_type.name,
                &intf_type.implements_interfaces,
                &intf_type.fields,
            ),
            _ => continue,
        };
        
        for interface_name in implements_interfaces {
               if let Some(ExtendedType::Interface(interface_type)) = schema.types.get(&interface_name.name) {
                for (field_name, _interface_field) in &interface_type.fields {
                    if !type_fields.contains_key(field_name) {
                        errors.push(CompositionError::InterfaceObjectUsageError {
                            message: format!(
                                "Type \"{}\" implements interface \"{}\" but is missing field \"{}\"",
                                type_name, interface_name, field_name
                            ),
                        });
                    }
                }
            }
        }
    }
}

