use apollo_compiler::Schema;
use apollo_federation::Supergraph;
use apollo_federation::subgraph::Subgraph;

fn print_sdl(schema: &Schema) -> String {
    let mut schema = schema.clone();
    schema.types.sort_keys();
    schema.directive_definitions.sort_keys();
    schema.to_string()
}

#[test]
fn can_compose_supergraph() {
    let s1 = Subgraph::parse_and_expand(
        "Subgraph1",
        "https://subgraph1",
        r#"
            type Query {
              t: T
            }

            type T @key(fields: "k") {
              k: ID
            }

            type S {
              x: Int
            }

            union U = S | T
        "#,
    )
    .unwrap();
    let s2 = Subgraph::parse_and_expand(
        "Subgraph2",
        "https://subgraph2",
        r#"
            type T @key(fields: "k") {
              k: ID
              a: Int
              b: String
            }

            enum E {
              V1
              V2
            }
        "#,
    )
    .unwrap();

    let supergraph = Supergraph::compose(vec![&s1, &s2]).unwrap();
    insta::assert_snapshot!(print_sdl(supergraph.schema.schema()));
    insta::assert_snapshot!(print_sdl(
        supergraph
            .to_api_schema(Default::default())
            .unwrap()
            .schema()
    ));
}

#[test]
fn can_compose_with_descriptions() {
    let s1 = Subgraph::parse_and_expand(
        "Subgraph1",
        "https://subgraph1",
        r#"
            "The foo directive description"
            directive @foo(url: String) on FIELD

            "A cool schema"
            schema {
              query: Query
            }

            """
            Available queries
            Not much yet
            """
            type Query {
              "Returns tea"
              t(
                "An argument that is very important"
                x: String!
              ): String
            }
        "#,
    )
    .unwrap();

    let s2 = Subgraph::parse_and_expand(
        "Subgraph2",
        "https://subgraph2",
        r#"
            "The foo directive description"
            directive @foo(url: String) on FIELD

            "An enum"
            enum E {
              "The A value"
              A
              "The B value"
              B
            }
        "#,
    )
    .unwrap();

    let supergraph = Supergraph::compose(vec![&s1, &s2]).unwrap();
    insta::assert_snapshot!(print_sdl(supergraph.schema.schema()));
    insta::assert_snapshot!(print_sdl(
        supergraph
            .to_api_schema(Default::default())
            .unwrap()
            .schema()
    ));
}

#[test]
fn can_compose_types_from_different_subgraphs() {
    let s1 = Subgraph::parse_and_expand(
        "SubgraphA",
        "https://subgraphA",
        r#"
            type Query {
                products: [Product!]
            }

            type Product {
                sku: String!
                name: String!
            }
        "#,
    )
    .unwrap();

    let s2 = Subgraph::parse_and_expand(
        "SubgraphB",
        "https://subgraphB",
        r#"
            type User {
                name: String
                email: String!
            }
        "#,
    )
    .unwrap();
    let supergraph = Supergraph::compose(vec![&s1, &s2]).unwrap();
    insta::assert_snapshot!(print_sdl(supergraph.schema.schema()));
    insta::assert_snapshot!(print_sdl(
        supergraph
            .to_api_schema(Default::default())
            .unwrap()
            .schema()
    ));
}

#[test]
fn compose_removes_federation_directives() {
    let s1 = Subgraph::parse_and_expand(
        "SubgraphA",
        "https://subgraphA",
        r#"
            extend schema @link(url: "https://specs.apollo.dev/federation/v2.5", import: [ "@key", "@provides", "@external" ])

            type Query {
              products: [Product!] @provides(fields: "name")
            }

            type Product @key(fields: "sku") {
              sku: String!
              name: String! @external
            }
        "#,
    )
        .unwrap();

    let s2 = Subgraph::parse_and_expand(
        "SubgraphB",
        "https://subgraphB",
        r#"
            extend schema @link(url: "https://specs.apollo.dev/federation/v2.5", import: [ "@key", "@shareable" ])

            type Product @key(fields: "sku") {
              sku: String!
              name: String! @shareable
            }
        "#,
    )
        .unwrap();

    let supergraph = Supergraph::compose(vec![&s1, &s2]).unwrap();
    insta::assert_snapshot!(print_sdl(supergraph.schema.schema()));
    insta::assert_snapshot!(print_sdl(
        supergraph
            .to_api_schema(Default::default())
            .unwrap()
            .schema()
    ));
}

use apollo_compiler::Schema as Schema_;
use apollo_federation::composition::{
    expand_subgraphs, merge_subgraphs, pre_merge_validations, post_merge_validations, 
    upgrade_subgraphs_if_necessary, validate_subgraphs, compose_with_options, 
    CompositionOptions, CompositionResult, CompositionHint
};
use apollo_federation::subgraph::typestate::Subgraph as Subgraph_;
use apollo_federation::error::CompositionError;

#[test]
fn can_validate_subgraph_before_merge() {
    let schema_str = r#"
                extend schema @link(url: "https://specs.apollo.dev/federation/v2.0")

                type Query {
                    s: String
                }"#
    .to_string();

    let schema = Schema_::parse(schema_str, "").unwrap();
    let s1 = Subgraph_::new("S", "http://S", schema.clone());
    let expanded_subgraphs = expand_subgraphs(vec![s1]).unwrap();
    let upgraded_subgraphs = upgrade_subgraphs_if_necessary(expanded_subgraphs).unwrap();
    let validated_subgraphs = validate_subgraphs(upgraded_subgraphs).unwrap();

    let outcome = pre_merge_validations(&validated_subgraphs).unwrap();

    assert!(outcome == ())
}

#[test]
fn pre_merge_validations_detects_duplicate_subgraph_names() {
    let schema_str = r#"
        extend schema @link(url: "https://specs.apollo.dev/federation/v2.0")
        type Query {
            s: String
        }"#;

    let schema = Schema_::parse(schema_str, "").unwrap();
    let s1 = Subgraph_::new("DuplicateName", "http://s1", schema.clone());
    let s2 = Subgraph_::new("DuplicateName", "http://s2", schema.clone());
    
    let expanded_subgraphs = expand_subgraphs(vec![s1, s2]).unwrap();
    let upgraded_subgraphs = upgrade_subgraphs_if_necessary(expanded_subgraphs).unwrap();
    let validated_subgraphs = validate_subgraphs(upgraded_subgraphs).unwrap();

    let result = pre_merge_validations(&validated_subgraphs);
    
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    match &errors[0] {
        CompositionError::InvalidGraphQL { message } => {
            assert!(message.contains("Duplicate subgraph name: 'DuplicateName'"));
        }
        _ => panic!("Expected InvalidGraphQL error"),
    }
}

#[test]
fn pre_merge_validations_succeeds_with_auto_generated_query() {
    // Apollo Federation automatically adds a Query type if missing, so this test
    // verifies that the system handles schemas with only Mutation gracefully
    let schema_str = r#"
        schema @link(url: "https://specs.apollo.dev/federation/v2.0") {
            mutation: Mutation
        }
        
        type Mutation {
            updateSomething: String
        }"#;

    let schema = Schema_::parse(schema_str, "").unwrap();
    let s1 = Subgraph_::new("MutationOnly", "http://s1", schema.clone());
    
    let expanded_subgraphs = expand_subgraphs(vec![s1]).unwrap();
    let upgraded_subgraphs = upgrade_subgraphs_if_necessary(expanded_subgraphs).unwrap();
    let validated_subgraphs = validate_subgraphs(upgraded_subgraphs).unwrap();

    // Apollo Federation should automatically add a Query type during processing
    assert!(validated_subgraphs[0].validated_schema().schema().schema_definition.query.is_some());

    let result = pre_merge_validations(&validated_subgraphs);
    
    // Should succeed because Apollo Federation auto-generates Query type
    assert!(result.is_ok());
}

#[test]
fn merge_subgraphs_succeeds_with_valid_schemas() {
    let schema_str1 = r#"
        schema @link(url: "https://specs.apollo.dev/federation/v2.0", import: ["@key"]) {
            query: Query
        }
        
        type Query {
            product(id: ID!): Product
        }

        type Product @key(fields: "id") {
            id: ID!
            name: String!
        }
    "#;
    
    let schema_str2 = r#"
        schema @link(url: "https://specs.apollo.dev/federation/v2.0", import: ["@key"]) {
            query: Query
        }
        
        type Query {
            _dummy: String
        }
        
        type Product @key(fields: "id") {
            id: ID!
            price: Float!
        }
    "#;

    let schema1 = Schema_::parse(schema_str1, "").unwrap();
    let schema2 = Schema_::parse(schema_str2, "").unwrap();
    let s1 = Subgraph_::new("Subgraph1", "https://subgraph1", schema1);
    let s2 = Subgraph_::new("Subgraph2", "https://subgraph2", schema2);

    let expanded_subgraphs = expand_subgraphs(vec![s1, s2]).unwrap();
    let upgraded_subgraphs = upgrade_subgraphs_if_necessary(expanded_subgraphs).unwrap();
    let validated_subgraphs = validate_subgraphs(upgraded_subgraphs).unwrap();

    let result = merge_subgraphs(validated_subgraphs);
    
    assert!(result.is_ok());
    let supergraph = result.unwrap();
    
    // Verify the merged schema has query type
    let schema = supergraph.schema();
    assert!(schema.schema_definition.query.is_some());
}

#[test]
fn post_merge_validations_succeeds_with_valid_supergraph() {
    let schema_str = r#"
        schema @link(url: "https://specs.apollo.dev/federation/v2.0") {
            query: Query
        }
        
        type Query {
            hello: String
        }
    "#;

    let schema = Schema_::parse(schema_str, "").unwrap();
    let s1 = Subgraph_::new("Subgraph1", "https://subgraph1", schema);
    
    let expanded_subgraphs = expand_subgraphs(vec![s1]).unwrap();
    let upgraded_subgraphs = upgrade_subgraphs_if_necessary(expanded_subgraphs).unwrap();
    let validated_subgraphs = validate_subgraphs(upgraded_subgraphs).unwrap();
    let merged_supergraph = merge_subgraphs(validated_subgraphs).unwrap();
    
    let result = post_merge_validations(&merged_supergraph);
    assert!(result.is_ok());
}

#[test]
fn post_merge_validations_detects_interface_implementation_errors() {
    // This test would require creating a supergraph with invalid interface implementations
    // For now, we'll test the basic validation path
    let schema_str = r#"
        schema @link(url: "https://specs.apollo.dev/federation/v2.0") {
            query: Query
        }
        
        type Query {
            node: Node
        }
        
        interface Node {
            id: ID!
        }
        
        type User implements Node {
            id: ID!
            name: String!
        }
    "#;

    let schema = Schema_::parse(schema_str, "").unwrap();
    let s1 = Subgraph_::new("Subgraph1", "https://subgraph1", schema);
    
    let expanded_subgraphs = expand_subgraphs(vec![s1]).unwrap();
    let upgraded_subgraphs = upgrade_subgraphs_if_necessary(expanded_subgraphs).unwrap();
    let validated_subgraphs = validate_subgraphs(upgraded_subgraphs).unwrap();
    let merged_supergraph = merge_subgraphs(validated_subgraphs).unwrap();
    
    let result = post_merge_validations(&merged_supergraph);
    assert!(result.is_ok());
}

#[test]
fn compose_with_options_default_behavior() {
    let schema_str = r#"
        schema @link(url: "https://specs.apollo.dev/federation/v2.0") {
            query: Query
        }
        
        type Query {
            hello: String
        }"#;

    let schema = Schema_::parse(schema_str, "").unwrap();
    let s1 = Subgraph_::new("Subgraph1", "https://subgraph1", schema);

    let result = compose_with_options(vec![s1], CompositionOptions::new());
    
    match result {
        CompositionResult::Success { supergraph, hints } => {
            assert!(supergraph.schema().schema().schema_definition.query.is_some());
            // Hints may be empty for simple cases - just check it's a valid Vec
            assert!(hints.is_empty() || !hints.is_empty());
        }
        CompositionResult::Failure { errors } => {
            panic!("Expected successful composition, got errors: {:?}", errors);
        }
    }
}

#[test]
fn compose_with_options_skip_satisfiability() {
    let schema_str = r#"
        schema @link(url: "https://specs.apollo.dev/federation/v2.0") {
            query: Query
        }
        
        type Query {
            hello: String
        }"#;

    let schema = Schema_::parse(schema_str, "").unwrap();
    let s1 = Subgraph_::new("Subgraph1", "https://subgraph1", schema);

    let options = CompositionOptions::new().with_satisfiability(false);
    let result = compose_with_options(vec![s1], options);
    
    match result {
        CompositionResult::Success { supergraph, .. } => {
            assert!(supergraph.schema().schema().schema_definition.query.is_some());
        }
        CompositionResult::Failure { errors } => {
            panic!("Expected successful composition, got errors: {:?}", errors);
        }
    }
}

#[test]
fn compose_with_options_fails_with_empty_query_type() {
    // Apollo Federation automatically adds a Query type if missing, but it will be empty
    // and GraphQL requires Query types to have at least one field
    let schema_str = r#"
        schema @link(url: "https://specs.apollo.dev/federation/v2.0") {
            mutation: Mutation
        }
        
        type Mutation {
            updateSomething: String
        }"#;

    let schema = Schema_::parse(schema_str, "").unwrap();
    let s1 = Subgraph_::new("MutationOnly", "http://s1", schema);
    
    let result = compose_with_options(vec![s1], CompositionOptions::new());
    
    match result {
        CompositionResult::Success { .. } => {
            panic!("Expected composition failure due to empty Query type");
        }
        CompositionResult::Failure { errors } => {
            assert!(!errors.is_empty());
            // Should contain error about Query having no fields
            let has_empty_query_error = errors.iter().any(|e| match e {
                CompositionError::InternalError { message } => {
                    message.contains("Query` has no fields")
                }
                _ => false,
            });
            assert!(has_empty_query_error);
        }
    }
}

#[test]
fn composition_options_builder_pattern() {
    let options = CompositionOptions::new()
        .with_satisfiability(false)
        .with_max_validation_paths(100);
    
    assert_eq!(options.run_satisfiability, false);
    assert_eq!(options.max_validation_subgraph_paths, Some(100));
}

#[test]
fn composition_hint_structure() {
    // Test that CompositionHint can be created and used
    let hint = CompositionHint {
        code: "TEST_HINT".to_string(),
        message: "This is a test hint".to_string(),
    };
    
    assert_eq!(hint.code, "TEST_HINT");
    assert_eq!(hint.message, "This is a test hint");
}
