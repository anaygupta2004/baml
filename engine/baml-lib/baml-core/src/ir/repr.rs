use std::collections::HashSet;

use anyhow::{anyhow, Result};
use baml_types::{Constraint, ConstraintLevel, FieldType};
use either::Either;
use indexmap::{IndexMap, IndexSet};
use internal_baml_parser_database::{
    walkers::{
        ClassWalker, ClientSpec as AstClientSpec, ClientWalker, ConfigurationWalker,
        EnumValueWalker, EnumWalker, FieldWalker, FunctionWalker, TemplateStringWalker,
    },
    Attributes, ParserDatabase, PromptAst, RetryPolicyStrategy,
};
use internal_baml_schema_ast::ast::SubType;

use baml_types::JinjaExpression;
use internal_baml_schema_ast::ast::{self, FieldArity, WithName, WithSpan};
use serde::Serialize;

use crate::Configuration;

/// This class represents the intermediate representation of the BAML AST.
/// It is a representation of the BAML AST that is easier to work with than the
/// raw BAML AST, and should include all information necessary to generate
/// code in any target language.
#[derive(serde::Serialize, Debug)]
pub struct IntermediateRepr {
    enums: Vec<Node<Enum>>,
    classes: Vec<Node<Class>>,
    /// Strongly connected components of the dependency graph (finite cycles).
    finite_recursive_cycles: Vec<IndexSet<String>>,
    functions: Vec<Node<Function>>,
    clients: Vec<Node<Client>>,
    retry_policies: Vec<Node<RetryPolicy>>,
    template_strings: Vec<Node<TemplateString>>,

    #[serde(skip)]
    configuration: Configuration,
}

/// A generic walker. Only walkers instantiated with a concrete ID type (`I`) are useful.
#[derive(Clone, Copy)]
pub struct Walker<'db, I> {
    /// The parser database being traversed.
    pub db: &'db IntermediateRepr,
    /// The identifier of the focused element.
    pub item: I,
}

impl IntermediateRepr {
    pub fn create_empty() -> IntermediateRepr {
        IntermediateRepr {
            enums: vec![],
            classes: vec![],
            finite_recursive_cycles: vec![],
            functions: vec![],
            clients: vec![],
            retry_policies: vec![],
            template_strings: vec![],
            configuration: Configuration::new(),
        }
    }

    pub fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    pub fn required_env_vars(&self) -> HashSet<String> {
        // TODO: We should likely check the full IR.
        let mut env_vars = HashSet::new();

        for client in self.walk_clients() {
            client.required_env_vars().iter().for_each(|v| {
                env_vars.insert(v.to_string());
            });
        }

        // self.walk_functions().filter_map(
        //     |f| f.client_name()
        // ).map(|c| c.required_env_vars())

        // // for any functions, check for shorthand env vars
        // self.functions
        //     .iter()
        //     .filter_map(|f| f.elem.configs())
        //     .into_iter()
        //     .flatten()
        //     .flat_map(|(expr)| expr.client.required_env_vars())
        //     .collect()
        env_vars
    }

    /// Returns a list of all the recursive cycles in the IR.
    ///
    /// Each cycle is represented as a set of strings, where each string is the
    /// name of a class.
    pub fn finite_recursive_cycles(&self) -> &[IndexSet<String>] {
        &self.finite_recursive_cycles
    }

    pub fn walk_enums<'a>(&'a self) -> impl ExactSizeIterator<Item = Walker<'a, &'a Node<Enum>>> {
        self.enums.iter().map(|e| Walker { db: self, item: e })
    }

    pub fn walk_classes<'a>(
        &'a self,
    ) -> impl ExactSizeIterator<Item = Walker<'a, &'a Node<Class>>> {
        self.classes.iter().map(|e| Walker { db: self, item: e })
    }

    pub fn function_names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.functions.iter().map(|f| f.elem.name())
    }

    pub fn walk_functions<'a>(
        &'a self,
    ) -> impl ExactSizeIterator<Item = Walker<'a, &'a Node<Function>>> {
        self.functions.iter().map(|e| Walker { db: self, item: e })
    }

    pub fn walk_tests<'a>(
        &'a self,
    ) -> impl Iterator<Item = Walker<'a, (&'a Node<Function>, &'a Node<TestCase>)>> {
        self.functions.iter().flat_map(move |f| {
            f.elem.tests().iter().map(move |t| Walker {
                db: self,
                item: (f, t),
            })
        })
    }

    pub fn walk_clients<'a>(
        &'a self,
    ) -> impl ExactSizeIterator<Item = Walker<'a, &'a Node<Client>>> {
        self.clients.iter().map(|e| Walker { db: self, item: e })
    }

    pub fn walk_template_strings<'a>(
        &'a self,
    ) -> impl ExactSizeIterator<Item = Walker<'a, &'a Node<TemplateString>>> {
        self.template_strings
            .iter()
            .map(|e| Walker { db: self, item: e })
    }

    #[allow(dead_code)]
    pub fn walk_retry_policies<'a>(
        &'a self,
    ) -> impl ExactSizeIterator<Item = Walker<'a, &'a Node<RetryPolicy>>> {
        self.retry_policies
            .iter()
            .map(|e| Walker { db: self, item: e })
    }

    pub fn from_parser_database(
        db: &ParserDatabase,
        configuration: Configuration,
    ) -> Result<IntermediateRepr> {
        let mut repr = IntermediateRepr {
            enums: db
                .walk_enums()
                .map(|e| e.node(db))
                .collect::<Result<Vec<_>>>()?,
            classes: db
                .walk_classes()
                .map(|e| e.node(db))
                .collect::<Result<Vec<_>>>()?,
            finite_recursive_cycles: db
                .finite_recursive_cycles()
                .iter()
                .map(|ids| {
                    ids.iter()
                        .map(|id| db.ast()[*id].name().to_string())
                        .collect()
                })
                .collect(),
            functions: db
                .walk_functions()
                .map(|e| e.node(db))
                .collect::<Result<Vec<_>>>()?,
            clients: db
                .walk_clients()
                .map(|e| e.node(db))
                .collect::<Result<Vec<_>>>()?,
            retry_policies: db
                .walk_retry_policies()
                .map(|e| WithRepr::<RetryPolicy>::node(&e, db))
                .collect::<Result<Vec<_>>>()?,
            template_strings: db
                .walk_templates()
                .map(|e| e.node(db))
                .collect::<Result<Vec<_>>>()?,
            configuration,
        };

        // Sort each item by name.
        repr.enums.sort_by(|a, b| a.elem.name.cmp(&b.elem.name));
        repr.classes.sort_by(|a, b| a.elem.name.cmp(&b.elem.name));
        repr.functions
            .sort_by(|a, b| a.elem.name().cmp(&b.elem.name()));
        repr.clients.sort_by(|a, b| a.elem.name.cmp(&b.elem.name));
        repr.retry_policies
            .sort_by(|a, b| a.elem.name.0.cmp(&b.elem.name.0));

        Ok(repr)
    }
}

// TODO:
//
//   [x] clients - need to finish expressions
//   [x] metadata per node (attributes, spans, etc)
//           block-level attributes on enums, classes
//           field-level attributes on enum values, class fields
//           overrides can only exist in impls
//   [x] FieldArity (optional / required) needs to be handled
//   [x] other types of identifiers?
//   [ ] `baml update` needs to update lockfile right now
//          but baml CLI is installed globally
//   [ ] baml configuration - retry policies, generator, etc
//          [x] retry policies
//   [x] rename lockfile/mod.rs to ir/mod.rs
//   [x] wire Result<> type through, need this to be more sane

#[derive(Debug, serde::Serialize)]
pub struct NodeAttributes {
    /// Map of attributes on the corresponding IR node.
    ///
    /// Some follow special conventions:
    ///
    ///   - @skip becomes ("skip", bool)
    ///   - @alias(...) becomes ("alias", ...)
    #[serde(with = "indexmap::map::serde_seq")]
    meta: IndexMap<String, Expression>,

    pub constraints: Vec<Constraint>,

    // Spans
    #[serde(skip)]
    pub span: Option<ast::Span>,
}

impl NodeAttributes {
    pub fn get(&self, key: &str) -> Option<&Expression> {
        self.meta.get(key)
    }
}

impl Default for NodeAttributes {
    fn default() -> Self {
        NodeAttributes {
            meta: IndexMap::new(),
            constraints: Vec::new(),
            span: None,
        }
    }
}

fn to_ir_attributes(
    db: &ParserDatabase,
    maybe_ast_attributes: Option<&Attributes>,
) -> (IndexMap<String, Expression>, Vec<Constraint>) {
    let null_result = (IndexMap::new(), Vec::new());
    maybe_ast_attributes.map_or(null_result, |attributes| {
        let Attributes {
            description,
            alias,
            dynamic_type,
            skip,
            constraints,
        } = attributes;
        let description = description.as_ref().and_then(|d| {
            let name = "description".to_string();
            match d {
                ast::Expression::StringValue(s, _) => Some((name, Expression::String(s.clone()))),
                ast::Expression::RawStringValue(s) => {
                    Some((name, Expression::RawString(s.value().to_string())))
                }
                ast::Expression::JinjaExpressionValue(j, _) => {
                    Some((name, Expression::JinjaExpression(j.clone())))
                }
                _ => {
                    eprintln!("Warning, encountered an unexpected description attribute");
                    None
                }
            }
        });
        let alias = alias
            .as_ref()
            .map(|v| ("alias".to_string(), Expression::String(db[*v].to_string())));
        let dynamic_type = dynamic_type.as_ref().and_then(|v| {
            if *v {
                Some(("dynamic_type".to_string(), Expression::Bool(true)))
            } else {
                None
            }
        });
        let skip = skip.as_ref().and_then(|v| {
            if *v {
                Some(("skip".to_string(), Expression::Bool(true)))
            } else {
                None
            }
        });

        let meta = vec![description, alias, dynamic_type, skip]
            .into_iter()
            .filter_map(|s| s)
            .collect();
        (meta, constraints.clone())
    })
}

/// Nodes allow attaching metadata to a given IR entity: attributes, source location, etc
#[derive(serde::Serialize, Debug)]
pub struct Node<T> {
    pub attributes: NodeAttributes,
    pub elem: T,
}

/// Implement this for every node in the IR AST, where T is the type of IR node
pub trait WithRepr<T> {
    /// Represents block or field attributes - @@ for enums and classes, @ for enum values and class fields
    fn attributes(&self, _: &ParserDatabase) -> NodeAttributes {
        NodeAttributes {
            meta: IndexMap::new(),
            constraints: Vec::new(),
            span: None,
        }
    }

    fn repr(&self, db: &ParserDatabase) -> Result<T>;

    fn node(&self, db: &ParserDatabase) -> Result<Node<T>> {
        Ok(Node {
            elem: self.repr(db)?,
            attributes: self.attributes(db),
        })
    }
}

fn type_with_arity(t: FieldType, arity: &FieldArity) -> FieldType {
    match arity {
        FieldArity::Required => t,
        FieldArity::Optional => FieldType::Optional(Box::new(t)),
    }
}

impl WithRepr<FieldType> for ast::FieldType {
    // TODO: (Greg) This code only extracts constraints, and ignores any
    // other types of attributes attached to the type directly.
    fn attributes(&self, _db: &ParserDatabase) -> NodeAttributes {
        let constraints = self
            .attributes()
            .iter()
            .filter_map(|attr| {
                let level = match attr.name.to_string().as_str() {
                    "assert" => Some(ConstraintLevel::Assert),
                    "check" => Some(ConstraintLevel::Check),
                    _ => None,
                }?;
                let (label, expression) = match attr.arguments.arguments.as_slice() {
                    [arg1, arg2] => match (arg1.clone().value, arg2.clone().value) {
                        (
                            ast::Expression::Identifier(ast::Identifier::Local(s, _)),
                            ast::Expression::JinjaExpressionValue(j, _),
                        ) => Some((Some(s), j)),
                        _ => None,
                    },
                    [arg1] => match arg1.clone().value {
                        ast::Expression::JinjaExpressionValue(JinjaExpression(j), _) => {
                            Some((None, JinjaExpression(j.clone())))
                        }
                        _ => None,
                    },
                    _ => None,
                }?;
                Some(Constraint {
                    level,
                    expression,
                    label,
                })
            })
            .collect::<Vec<Constraint>>();
        let attributes = NodeAttributes {
            meta: IndexMap::new(),
            constraints,
            span: Some(self.span().clone()),
        };

        attributes
    }

    fn repr(&self, db: &ParserDatabase) -> Result<FieldType> {
        let constraints = WithRepr::attributes(self, db).constraints;
        let has_constraints = constraints.len() > 0;
        let base = match self {
            ast::FieldType::Primitive(arity, typeval, ..) => {
                let repr = FieldType::Primitive(typeval.clone());
                if arity.is_optional() {
                    FieldType::Optional(Box::new(repr))
                } else {
                    repr
                }
            }
            ast::FieldType::Literal(arity, literal_value, ..) => {
                let repr = FieldType::Literal(literal_value.clone());
                if arity.is_optional() {
                    FieldType::Optional(Box::new(repr))
                } else {
                    repr
                }
            }
            ast::FieldType::Symbol(arity, idn, ..) => type_with_arity(
                match db.find_type(idn) {
                    Some(Either::Left(class_walker)) => {
                        let base_class = FieldType::Class(class_walker.name().to_string());
                        let maybe_constraints = class_walker.get_constraints(SubType::Class);
                        match maybe_constraints {
                            Some(constraints) if constraints.len() > 0 => FieldType::Constrained {
                                base: Box::new(base_class),
                                constraints,
                            },
                            _ => base_class,
                        }
                    }
                    Some(Either::Right(enum_walker)) => {
                        let base_type = FieldType::Enum(enum_walker.name().to_string());
                        let maybe_constraints = enum_walker.get_constraints(SubType::Enum);
                        match maybe_constraints {
                            Some(constraints) if constraints.len() > 0 => FieldType::Constrained {
                                base: Box::new(base_type),
                                constraints,
                            },
                            _ => base_type,
                        }
                    }
                    None => return Err(anyhow!("Field type uses unresolvable local identifier")),
                },
                arity,
            ),
            ast::FieldType::List(arity, ft, dims, ..) => {
                // NB: potential bug: this hands back a 1D list when dims == 0
                let mut repr = FieldType::List(Box::new(ft.repr(db)?));

                for _ in 1u32..*dims {
                    repr = FieldType::list(repr);
                }

                if arity.is_optional() {
                    repr = FieldType::optional(repr);
                }

                repr
            }
            ast::FieldType::Map(arity, kv, ..) => {
                // NB: we can't just unpack (*kv) into k, v because that would require a move/copy
                let mut repr =
                    FieldType::Map(Box::new((*kv).0.repr(db)?), Box::new((*kv).1.repr(db)?));

                if arity.is_optional() {
                    repr = FieldType::optional(repr);
                }

                repr
            }
            ast::FieldType::Union(arity, t, ..) => {
                // NB: preempt union flattening by mixing arity into union types
                let mut types = t.iter().map(|ft| ft.repr(db)).collect::<Result<Vec<_>>>()?;

                if arity.is_optional() {
                    types.push(FieldType::Primitive(baml_types::TypeValue::Null));
                }

                FieldType::Union(types)
            }
            ast::FieldType::Tuple(arity, t, ..) => type_with_arity(
                FieldType::Tuple(t.iter().map(|ft| ft.repr(db)).collect::<Result<Vec<_>>>()?),
                arity,
            ),
        };

        let with_constraints = if has_constraints {
            FieldType::Constrained {
                base: Box::new(base.clone()),
                constraints,
            }
        } else {
            base
        };
        Ok(with_constraints)
    }
}

#[derive(serde::Serialize, Debug)]
pub enum Identifier {
    /// Starts with env.*
    ENV(String),
    /// The path to a Local Identifer + the local identifer. Separated by '.'
    #[allow(dead_code)]
    Ref(Vec<String>),
    /// A string without spaces or '.' Always starts with a letter. May contain numbers
    Local(String),
    /// Special types (always lowercase).
    Primitive(baml_types::TypeValue),
}

impl Identifier {
    pub fn name(&self) -> String {
        match self {
            Identifier::ENV(k) => k.clone(),
            Identifier::Ref(r) => r.join("."),
            Identifier::Local(l) => l.clone(),
            Identifier::Primitive(p) => p.to_string(),
        }
    }
}

#[derive(serde::Serialize, Debug)]
pub enum Expression {
    Identifier(Identifier),
    Bool(bool),
    Numeric(String),
    String(String),
    RawString(String),
    List(Vec<Expression>),
    Map(Vec<(Expression, Expression)>),
    JinjaExpression(JinjaExpression),
}

impl Expression {
    pub fn required_env_vars<'a>(&'a self) -> Vec<String> {
        match self {
            Expression::Identifier(Identifier::ENV(k)) => vec![k.to_string()],
            Expression::List(l) => l.iter().flat_map(Expression::required_env_vars).collect(),
            Expression::Map(m) => m
                .iter()
                .flat_map(|(k, v)| {
                    let mut keys = k.required_env_vars();
                    keys.extend(v.required_env_vars());
                    keys
                })
                .collect(),
            _ => vec![],
        }
    }
}

impl WithRepr<Expression> for ast::Expression {
    fn repr(&self, db: &ParserDatabase) -> Result<Expression> {
        Ok(match self {
            ast::Expression::BoolValue(val, _) => Expression::Bool(val.clone()),
            ast::Expression::NumericValue(val, _) => Expression::Numeric(val.clone()),
            ast::Expression::StringValue(val, _) => Expression::String(val.clone()),
            ast::Expression::RawStringValue(val) => Expression::RawString(val.value().to_string()),
            ast::Expression::JinjaExpressionValue(val, _) => {
                Expression::JinjaExpression(val.clone())
            }
            ast::Expression::Identifier(idn) => match idn {
                ast::Identifier::ENV(k, _) => {
                    Ok(Expression::Identifier(Identifier::ENV(k.clone())))
                }
                ast::Identifier::String(s, _) => Ok(Expression::String(s.clone())),
                ast::Identifier::Local(l, _) => {
                    Ok(Expression::Identifier(Identifier::Local(l.clone())))
                }
                ast::Identifier::Ref(r, _) => {
                    // NOTE(sam): this feels very very wrong, but per vbv, we don't really use refs
                    // right now, so this should be safe. this is done to ensure that
                    // "options { model gpt-3.5-turbo }" is represented correctly in the resulting IR,
                    // specifically that "gpt-3.5-turbo" is actually modelled as Expression::String
                    //
                    // this does not impact the handling of "options { api_key env.OPENAI_API_KEY }"
                    // because that's modelled as Identifier::ENV, not Identifier::Ref
                    Ok(Expression::String(r.full_name.clone()))
                }

                ast::Identifier::Invalid(_, _) => {
                    Err(anyhow!("Cannot represent an invalid parser-AST identifier"))
                }
            }?,
            ast::Expression::Array(arr, _) => {
                Expression::List(arr.iter().map(|e| e.repr(db)).collect::<Result<Vec<_>>>()?)
            }
            ast::Expression::Map(arr, _) => Expression::Map(
                arr.iter()
                    .map(|(k, v)| Ok((k.repr(db)?, v.repr(db)?)))
                    .collect::<Result<Vec<_>>>()?,
            ),
        })
    }
}

type TemplateStringId = String;

#[derive(serde::Serialize, Debug)]
pub struct TemplateString {
    pub name: TemplateStringId,
    pub params: Vec<Field>,
    pub content: String,
}

impl WithRepr<TemplateString> for TemplateStringWalker<'_> {
    fn attributes(&self, _: &ParserDatabase) -> NodeAttributes {
        NodeAttributes {
            meta: Default::default(),
            constraints: Vec::new(),
            span: Some(self.span().clone()),
        }
    }

    fn repr(&self, _db: &ParserDatabase) -> Result<TemplateString> {
        Ok(TemplateString {
            name: self.name().to_string(),
            params: self.ast_node().input().map_or(vec![], |e| match e {
                ast::BlockArgs { args, .. } => args
                    .iter()
                    .filter_map(|(id, arg)| {
                        arg.field_type
                            .node(_db)
                            .map(|f| Field {
                                name: id.name().to_string(),
                                r#type: f,
                            })
                            .ok()
                    })
                    .collect::<Vec<_>>(),
            }),
            content: self.template_string().to_string(),
        })
    }
}
type EnumId = String;

#[derive(serde::Serialize, Debug)]
pub struct EnumValue(pub String);

#[derive(serde::Serialize, Debug)]
pub struct Enum {
    pub name: EnumId,
    pub values: Vec<Node<EnumValue>>,
}

impl WithRepr<EnumValue> for EnumValueWalker<'_> {
    fn attributes(&self, db: &ParserDatabase) -> NodeAttributes {
        let (meta, constraints) = to_ir_attributes(db, self.get_default_attributes());
        let attributes = NodeAttributes {
            meta,
            constraints,
            span: Some(self.span().clone()),
        };

        attributes
    }

    fn repr(&self, _db: &ParserDatabase) -> Result<EnumValue> {
        Ok(EnumValue(self.name().to_string()))
    }
}

impl WithRepr<Enum> for EnumWalker<'_> {
    fn attributes(&self, db: &ParserDatabase) -> NodeAttributes {
        let (meta, constraints) = to_ir_attributes(db, self.get_default_attributes(SubType::Enum));
        let attributes = NodeAttributes {
            meta,
            constraints,
            span: Some(self.span().clone()),
        };

        attributes
    }

    fn repr(&self, db: &ParserDatabase) -> Result<Enum> {
        Ok(Enum {
            name: self.name().to_string(),
            values: self
                .values()
                .map(|v| v.node(db))
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

#[derive(serde::Serialize, Debug)]
pub struct Field {
    pub name: String,
    pub r#type: Node<FieldType>,
}

impl WithRepr<Field> for FieldWalker<'_> {
    fn attributes(&self, db: &ParserDatabase) -> NodeAttributes {
        let (meta, constraints) = to_ir_attributes(db, self.get_default_attributes());
        let attributes = NodeAttributes {
            meta,
            constraints,
            span: Some(self.span().clone()),
        };

        attributes
    }

    fn repr(&self, db: &ParserDatabase) -> Result<Field> {
        Ok(Field {
            name: self.name().to_string(),
            r#type: Node {
                elem: self
                    .ast_field()
                    .expr
                    .clone()
                    .ok_or(anyhow!(
                        "Internal error occurred while resolving repr of field {:?}",
                        self.name(),
                    ))?
                    .repr(db)?,
                attributes: self.attributes(db),
            },
        })
    }
}

type ClassId = String;

/// A BAML Class.
#[derive(serde::Serialize, Debug)]
pub struct Class {
    /// User defined class name.
    pub name: ClassId,

    /// Fields of the class.
    pub static_fields: Vec<Node<Field>>,

    /// Parameters to the class definition.
    pub inputs: Vec<(String, FieldType)>,
}

impl WithRepr<Class> for ClassWalker<'_> {
    fn attributes(&self, db: &ParserDatabase) -> NodeAttributes {
        let default_attributes = self.get_default_attributes(SubType::Class);
        let (meta, constraints) = to_ir_attributes(db, default_attributes);
        let attributes = NodeAttributes {
            meta,
            constraints,
            span: Some(self.span().clone()),
        };

        attributes
    }

    fn repr(&self, db: &ParserDatabase) -> Result<Class> {
        Ok(Class {
            name: self.name().to_string(),
            static_fields: self
                .static_fields()
                .map(|e| e.node(db))
                .collect::<Result<Vec<_>>>()?,
            inputs: match self.ast_type_block().input() {
                Some(input) => input
                    .args
                    .iter()
                    .map(|arg| {
                        let field_type = arg.1.field_type.repr(db)?;
                        Ok((arg.0.to_string(), field_type))
                    })
                    .collect::<Result<Vec<_>>>()?,
                None => Vec::new(),
            },
        })
    }
}

impl Class {
    pub fn inputs(&self) -> &Vec<(String, FieldType)> {
        &self.inputs
    }
}
#[derive(serde::Serialize, Debug)]
pub enum OracleType {
    LLM,
}
#[derive(serde::Serialize, Debug)]
pub struct AliasOverride {
    pub name: String,
    // This is used to generate deserializers with aliased keys (see .overload in python deserializer)
    pub aliased_keys: Vec<AliasedKey>,
}

// TODO, also add skips
#[derive(serde::Serialize, Debug)]
pub struct AliasedKey {
    pub key: String,
    pub alias: Expression,
}

type ImplementationId = String;

#[derive(serde::Serialize, Debug)]
pub struct Implementation {
    r#type: OracleType,
    pub name: ImplementationId,
    pub function_name: String,

    pub prompt: Prompt,

    #[serde(with = "indexmap::map::serde_seq")]
    pub input_replacers: IndexMap<String, String>,

    #[serde(with = "indexmap::map::serde_seq")]
    pub output_replacers: IndexMap<String, String>,

    pub client: ClientId,

    /// Inputs for deserializer.overload in the generated code.
    ///
    /// This is NOT 1:1 with "override" clauses in the .baml file.
    ///
    /// For enums, we generate one for "alias", one for "description", and one for "alias: description"
    /// (this means that we currently don't support deserializing "alias[^a-zA-Z0-9]{1,5}description" but
    /// for now it suffices)
    pub overrides: Vec<AliasOverride>,
}

/// BAML does not allow UnnamedArgList nor a lone NamedArg
#[derive(serde::Serialize, Debug)]
pub enum FunctionArgs {
    UnnamedArg(FieldType),
    NamedArgList(Vec<(String, FieldType)>),
}

type FunctionId = String;

impl Function {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn output(&self) -> &FieldType {
        &self.output
    }

    pub fn inputs(&self) -> &Vec<(String, FieldType)> {
        &self.inputs
    }

    pub fn tests(&self) -> &Vec<Node<TestCase>> {
        &self.tests
    }

    pub fn configs(&self) -> Option<&Vec<FunctionConfig>> {
        Some(&self.configs)
    }
}

#[derive(serde::Serialize, Debug)]
pub struct Function {
    pub name: FunctionId,
    pub inputs: Vec<(String, FieldType)>,
    pub output: FieldType,
    pub tests: Vec<Node<TestCase>>,
    pub configs: Vec<FunctionConfig>,
    pub default_config: String,
}

#[derive(serde::Serialize, Debug)]
pub struct FunctionConfig {
    pub name: String,
    pub prompt_template: String,
    #[serde(skip)]
    pub prompt_span: ast::Span,
    pub client: ClientSpec,
}

// NB(sam): we used to use this to bridge the wasm layer, but
// I don't think we do anymore.
#[derive(serde::Serialize, Clone, Debug)]
pub enum ClientSpec {
    Named(String),
    /// Shorthand for "<provider>/<model>"
    Shorthand(String, String),
}

impl ClientSpec {
    pub fn as_str(&self) -> String {
        match self {
            ClientSpec::Named(n) => n.clone(),
            ClientSpec::Shorthand(provider, model) => format!("{provider}/{model}"),
        }
    }

    pub fn new_from_id(arg: String) -> Self {
        if arg.contains("/") {
            let (provider, model) = arg.split_once("/").unwrap();
            ClientSpec::Shorthand(provider.to_string(), model.to_string())
        } else {
            ClientSpec::Named(arg)
        }
    }

    pub fn required_env_vars(&self) -> HashSet<String> {
        match self {
            ClientSpec::Named(n) => HashSet::new(),
            ClientSpec::Shorthand(_, _) => HashSet::new(),
        }
    }
}

impl From<AstClientSpec> for ClientSpec {
    fn from(spec: AstClientSpec) -> Self {
        match spec {
            AstClientSpec::Named(n) => ClientSpec::Named(n.to_string()),
            AstClientSpec::Shorthand(provider, model) => ClientSpec::Shorthand(provider, model),
        }
    }
}

impl std::fmt::Display for ClientSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

fn process_field(
    overrides: &IndexMap<(String, String), IndexMap<String, Expression>>, // Adjust the type according to your actual field type
    original_name: &str,
    function_name: &str,
    impl_name: &str,
) -> Vec<AliasedKey> {
    // This feeds into deserializer.overload; the registerEnumDeserializer counterpart is in generate_ts_client.rs
    match overrides.get(&((*function_name).to_string(), (*impl_name).to_string())) {
        Some(overrides) => {
            if let Some(Expression::String(alias)) = overrides.get("alias") {
                if let Some(Expression::String(description)) = overrides.get("description") {
                    // "alias" and "alias: description"
                    vec![
                        AliasedKey {
                            key: original_name.to_string(),
                            alias: Expression::String(alias.clone()),
                        },
                        AliasedKey {
                            key: original_name.to_string(),
                            alias: Expression::String(format!("{}: {}", alias, description)),
                        },
                    ]
                } else {
                    // "alias"
                    vec![AliasedKey {
                        key: original_name.to_string(),
                        alias: Expression::String(alias.clone()),
                    }]
                }
            } else if let Some(Expression::String(description)) = overrides.get("description") {
                // "description"
                vec![AliasedKey {
                    key: original_name.to_string(),
                    alias: Expression::String(description.clone()),
                }]
            } else {
                // no overrides
                vec![]
            }
        }
        None => Vec::new(),
    }
}

impl WithRepr<Function> for FunctionWalker<'_> {
    fn attributes(&self, _: &ParserDatabase) -> NodeAttributes {
        NodeAttributes {
            meta: Default::default(),
            constraints: Vec::new(),
            span: Some(self.span().clone()),
        }
    }

    fn repr(&self, db: &ParserDatabase) -> Result<Function> {
        Ok(Function {
            name: self.name().to_string(),
            inputs: self
                .ast_function()
                .input()
                .expect("msg")
                .args
                .iter()
                .map(|arg| {
                    let field_type = arg.1.field_type.repr(db)?;
                    Ok((arg.0.to_string(), field_type))
                })
                .collect::<Result<Vec<_>>>()?,
            output: self
                .ast_function()
                .output()
                .expect("need block arg")
                .field_type
                .repr(db)?,
            configs: vec![FunctionConfig {
                name: "default_config".to_string(),
                prompt_template: self.jinja_prompt().to_string(),
                prompt_span: self.ast_function().span().clone(),
                client: match self.client_spec() {
                    Ok(spec) => ClientSpec::from(spec),
                    Err(e) => anyhow::bail!("{}", e.message()),
                },
            }],
            default_config: "default_config".to_string(),
            tests: self
                .walk_tests()
                .map(|e| e.node(db))
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

type ClientId = String;

#[derive(serde::Serialize, Debug)]
pub struct Client {
    pub name: ClientId,
    pub provider: String,
    pub retry_policy_id: Option<String>,
    pub options: Vec<(String, Expression)>,
}

impl WithRepr<Client> for ClientWalker<'_> {
    fn attributes(&self, _: &ParserDatabase) -> NodeAttributes {
        NodeAttributes {
            meta: IndexMap::new(),
            constraints: Vec::new(),
            span: Some(self.span().clone()),
        }
    }

    fn repr(&self, db: &ParserDatabase) -> Result<Client> {
        Ok(Client {
            name: self.name().to_string(),
            provider: self.properties().provider.0.clone(),
            options: self
                .properties()
                .options
                .iter()
                .map(|(k, v)| Ok((k.clone(), v.repr(db)?)))
                .collect::<Result<Vec<_>>>()?,
            retry_policy_id: self
                .properties()
                .retry_policy
                .as_ref()
                .map(|(id, _)| id.clone()),
        })
    }
}

#[derive(serde::Serialize, Debug)]
pub struct RetryPolicyId(pub String);

#[derive(serde::Serialize, Debug)]
pub struct RetryPolicy {
    pub name: RetryPolicyId,
    pub max_retries: u32,
    pub strategy: RetryPolicyStrategy,
    // NB: the parser DB has a notion of "empty options" vs "no options"; we collapse
    // those here into an empty vec
    options: Vec<(String, Expression)>,
}

impl WithRepr<RetryPolicy> for ConfigurationWalker<'_> {
    fn attributes(&self, _db: &ParserDatabase) -> NodeAttributes {
        NodeAttributes {
            meta: IndexMap::new(),
            constraints: Vec::new(),
            span: Some(self.span().clone()),
        }
    }

    fn repr(&self, db: &ParserDatabase) -> Result<RetryPolicy> {
        Ok(RetryPolicy {
            name: RetryPolicyId(self.name().to_string()),
            max_retries: self.retry_policy().max_retries,
            strategy: self.retry_policy().strategy,
            options: match &self.retry_policy().options {
                Some(o) => o
                    .iter()
                    .map(|((k, _), v)| Ok((k.clone(), v.repr(db)?)))
                    .collect::<Result<Vec<_>>>()?,
                None => vec![],
            },
        })
    }
}

#[derive(serde::Serialize, Debug)]
pub struct TestCaseFunction(String);

impl TestCaseFunction {
    pub fn name(&self) -> &str {
        &self.0
    }
}

#[derive(serde::Serialize, Debug)]
pub struct TestCase {
    pub name: String,
    pub functions: Vec<Node<TestCaseFunction>>,
    pub args: IndexMap<String, Expression>,
}

impl WithRepr<TestCaseFunction> for (&ConfigurationWalker<'_>, usize) {
    fn attributes(&self, _db: &ParserDatabase) -> NodeAttributes {
        let span = self.0.test_case().functions[self.1].1.clone();
        NodeAttributes {
            meta: IndexMap::new(),
            constraints: Vec::new(),
            span: Some(span),
        }
    }

    fn repr(&self, _db: &ParserDatabase) -> Result<TestCaseFunction> {
        Ok(TestCaseFunction(
            self.0.test_case().functions[self.1].0.clone(),
        ))
    }
}

impl WithRepr<TestCase> for ConfigurationWalker<'_> {
    fn attributes(&self, _db: &ParserDatabase) -> NodeAttributes {
        NodeAttributes {
            meta: IndexMap::new(),
            span: Some(self.span().clone()),
            constraints: Vec::new(),
        }
    }

    fn repr(&self, db: &ParserDatabase) -> Result<TestCase> {
        let functions = (0..self.test_case().functions.len())
            .map(|i| (self, i).node(db))
            .collect::<Result<Vec<_>>>()?;
        Ok(TestCase {
            name: self.name().to_string(),
            args: self
                .test_case()
                .args
                .iter()
                .map(|(k, (_, v))| Ok((k.clone(), v.repr(db)?)))
                .collect::<Result<IndexMap<_, _>>>()?,
            functions,
        })
    }
}
#[derive(Debug, Clone, Serialize)]
pub enum Prompt {
    // The prompt stirng, and a list of input replacer keys (raw key w/ magic string, and key to replace with)
    String(String, Vec<(String, String)>),

    // same thing, the chat message, and the replacer input keys (raw key w/ magic string, and key to replace with)
    Chat(Vec<ChatMessage>, Vec<(String, String)>),
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct ChatMessage {
    pub idx: u32,
    pub role: String,
    pub content: String,
}

impl WithRepr<Prompt> for PromptAst<'_> {
    fn repr(&self, _db: &ParserDatabase) -> Result<Prompt> {
        Ok(match self {
            PromptAst::String(content, _) => Prompt::String(content.clone(), vec![]),
            PromptAst::Chat(messages, input_replacers) => Prompt::Chat(
                messages
                    .iter()
                    .filter_map(|(message, content)| {
                        message.as_ref().map(|m| ChatMessage {
                            idx: m.idx,
                            role: m.role.0.clone(),
                            content: content.clone(),
                        })
                    })
                    .collect::<Vec<_>>(),
                input_replacers.to_vec(),
            ),
        })
    }
}

/// Generate an IntermediateRepr from a single block of BAML source code.
/// This is useful for generating IR test fixtures.
pub fn make_test_ir(source_code: &str) -> anyhow::Result<IntermediateRepr> {
    use crate::validate;
    use crate::ValidatedSchema;
    use internal_baml_diagnostics::SourceFile;
    use std::path::PathBuf;

    let path: PathBuf = "fake_file.baml".into();
    let source_file: SourceFile = (path.clone(), source_code).into();
    let validated_schema: ValidatedSchema = validate(&path, vec![source_file]);
    let diagnostics = &validated_schema.diagnostics;
    if diagnostics.has_errors() {
        return Err(anyhow::anyhow!(
            "Source code was invalid: \n{:?}",
            diagnostics.errors()
        ));
    }
    let ir = IntermediateRepr::from_parser_database(
        &validated_schema.db,
        validated_schema.configuration,
    )?;
    Ok(ir)
}
