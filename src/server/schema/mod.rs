use bifrost::raft::RaftService;
use bifrost::raft::client::RaftClient;
use bifrost::raft::state_machine::master::ExecError;
use bifrost_hasher::hash_str;
use graph::edge::{EdgeAttributes, EdgeType};
use graph::edge;
use chashmap::CHashMap;
use std::sync::Arc;
use neb::ram::schema::{Field, Schema};
use neb::client::{AsyncClient as NebClient};
use neb::server::{ServerMeta as NebServerMeta};
use server::schema::sm::schema_types::client::SMClient;
use graph::fields::VERTEX_TEMPLATE;
use futures::{Future, future};

mod sm;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq)]
pub enum SchemaType {
    Unspecified,
    Vertex,
    Edge(EdgeAttributes)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SchemaError {
    NewNebSchemaExecError(ExecError),
    NewMorpheusSchemaExecError(ExecError),
    SimpleEdgeShouldNotHaveSchema,
    SchemaTypeUnspecified,
}

pub struct SchemaContainer {
    pub neb_client: Arc<NebClient>,
    map: Arc<CHashMap<u32, SchemaType>>,
    sm_client: Arc<SMClient>,
    neb_mata: Arc<NebServerMeta>,
}

#[derive(Clone)]
pub struct MorpheusSchema {
    pub id: u32,
    pub name: String,
    pub schema_type: SchemaType,
    pub key_field: Option<Vec<String>>,
    pub fields: Vec<Field>,
    pub is_dynamic: bool
}

lazy_static! {
    pub static ref EMPTY_FIELDS: Vec<Field> = Vec::new();
}

impl MorpheusSchema {
    pub fn new<'a>(name: & 'a str, key_field: Option<&Vec<String>>, fields: &Vec<Field>, is_dynamic: bool) -> MorpheusSchema {
        MorpheusSchema {
            id: 0,
            name: name.to_string(),
            key_field: key_field.cloned(),
            fields: fields.clone(),
            schema_type: SchemaType::Unspecified,
            is_dynamic
        }
    }
    pub fn into_ref(self) -> Arc<MorpheusSchema> {
        Arc::new(self)
    }
}

pub fn cell_fields(schema_type: SchemaType, mut body_fields: Vec<Field>) -> Result<Vec<Field>, SchemaError> {
    let mut fields = match schema_type {
        SchemaType::Vertex => VERTEX_TEMPLATE.clone(),
        SchemaType::Edge(edge_attr) => {
            if !edge_attr.has_body && body_fields.len() > 0 {
                return Err(SchemaError::SimpleEdgeShouldNotHaveSchema);
            }
            match edge_attr.edge_type {
                EdgeType::Directed => edge::directed::EDGE_TEMPLATE.clone(),
                EdgeType::Undirected => edge::undirectd::EDGE_TEMPLATE.clone(),
            }
        },
        SchemaType::Unspecified => return Err(SchemaError::SchemaTypeUnspecified)
    };
    fields.append(&mut body_fields);
    Ok(fields)
}

pub fn generate_sm_id<'a>(group: &'a str) -> u64 {
    hash_str(&format!("{}-{}", sm::DEFAULT_RAFT_PREFIX, group))
}

impl SchemaContainer {

    pub fn new_meta_service<'a>(group: &'a str, raft_service: &Arc<RaftService>) {
        let mut container_sm = sm::schema_types::Map::new(generate_sm_id(group));
        container_sm.init_callback(raft_service);
        raft_service.register_state_machine(Box::new(container_sm));
    }

    pub fn new_client<'a>(
        group: &'a str,
        raft_client: &Arc<RaftClient>,
        neb_client: &Arc<NebClient>,
        neb_meta: &Arc<NebServerMeta>
    ) -> Result<Arc<SchemaContainer>, ExecError> {
        let sm_client = Arc::new(SMClient::new(generate_sm_id(group), &raft_client));
        let sm_entries = sm_client.entries()?.unwrap();
        let container = SchemaContainer {
            map: Arc::new(CHashMap::new()),
            sm_client: sm_client.clone(),
            neb_client: neb_client.clone(),
            neb_mata: neb_meta.clone()
        };
        let container_ref = Arc::new(container);
        let container_ref1 = container_ref.clone();
        let container_ref2 = container_ref.clone();
        for (schema_id, schema_type) in sm_entries {
            container_ref.map.insert(schema_id, schema_type);
        }
        sm_client.on_inserted(move |res| {
            if let Ok((id, schema_type)) = res {
                container_ref1.map.insert(id, schema_type);
            }
        })?;
        sm_client.on_removed(move |res| {
            if let Ok((id, schema_type)) = res {
                container_ref2.map.remove(&id);
            }
        })?;
        return Ok(container_ref);
    }

    pub fn new_schema(&self, schema: MorpheusSchema) -> impl Future<Item = u32, Error = SchemaError> {
        let schema_type = schema.schema_type;
        let sm_client = self.sm_client.clone();
        let neb_client = self.neb_client.clone();
        future::result(cell_fields(schema_type, schema.fields.clone()))
            .and_then(move |schema_fields| {
                let mut neb_schema = Schema::new(
                    &schema.name,
                    schema.key_field.clone(),
                    Field::new(&String::from("*"), 0, false, false, Some(schema_fields)),
                    schema.is_dynamic
                );
                neb_client.new_schema(neb_schema)
                    .map_err(|e| SchemaError::NewNebSchemaExecError(e))
            })
            .and_then(move |(schema_id, _)| {
                match sm_client.insert(&schema_id, &schema_type) {
                    Ok(_) => Ok(schema_id),
                    Err(e) => Err(SchemaError::NewMorpheusSchemaExecError(e))
                }
            })
    }

    pub fn schema_type(&self, schema_id: u32) -> Option<SchemaType> {
        Self::schema_type_(&self.map, schema_id)
    }

    fn schema_type_(map: &Arc<CHashMap<u32, SchemaType>>, schema_id: u32) -> Option<SchemaType> {
        match map.get(&schema_id) {
            Some(t) => Some(*t),
            None => None
        }
    }

    pub fn id_from_name<'a>(&self, name : &'a str) -> Option<u32> {
        self.neb_mata.schemas.name_to_id(name)
    }

    pub fn from_name<'a>(&self, name: &'a str) -> Option<MorpheusSchema> {
        let schema_id = self.id_from_name(name).unwrap_or(0);
        match self.get_neb_schema(schema_id) {
            Some(neb_schema) => self.neb_to_morpheus_schema(&neb_schema), None => None
        }
    }

    pub fn get_neb_schema(&self, schema_id: u32) -> Option<Arc<Schema>> {
        self.neb_mata.schemas.get(&schema_id)
    }
    pub fn neb_to_morpheus_schema(&self, schema: &Arc<Schema>) -> Option<MorpheusSchema> {
        Self::neb_to_morpheus_schema_(&self.map, schema)
    }
    fn neb_to_morpheus_schema_(schema_map: &Arc<CHashMap<u32, SchemaType>>, schema: &Arc<Schema>) -> Option<MorpheusSchema> {
        if let Some(schema_type) = Self::schema_type_(schema_map, schema.id) {
            if let Some(ref fields) = schema.fields.sub_fields {
                Some(MorpheusSchema {
                    id: schema.id,
                    name: schema.name.clone(),
                    schema_type,
                    key_field: schema.str_key_field.clone(),
                    fields: fields.clone(),
                    is_dynamic: schema.is_dynamic
                })
            } else { None }
        } else { None }
    }
    pub fn all_morpheus_schemas(&self) -> impl Future<Item = Vec<MorpheusSchema>, Error = ExecError> {
        let schema_map = self.map.clone();
        self.neb_client.get_all_schema()
            .map(move |neb_schemas| {
                neb_schemas
                    .into_iter()
                    .map(|schema| Self::neb_to_morpheus_schema_(&schema_map, &Arc::new(schema)))
                    .filter_map(|ms| ms)
                    .collect()
            })
    }
    pub fn count(&self) -> impl Future<Item = usize, Error = ExecError> {
        self.all_morpheus_schemas().map(|x| x.len())
    }
}

pub trait ToSchemaId {
    fn to_id(&self, schemas: &Arc<SchemaContainer>) -> u32;
}

impl ToSchemaId for MorpheusSchema {
    fn to_id(&self, _: &Arc<SchemaContainer>) -> u32 {
        self.id
    }
}

impl ToSchemaId for u32 {
    fn to_id(&self, _: &Arc<SchemaContainer>) -> u32 {
        *self
    }
}

impl ToSchemaId for Schema {
    fn to_id(&self, _: &Arc<SchemaContainer>) -> u32 {
        self.id
    }
}

impl ToSchemaId for Arc<Schema> {
    fn to_id(&self, _: &Arc<SchemaContainer>) -> u32 {
        self.id
    }
}

impl ToSchemaId for Arc<MorpheusSchema> {
    fn to_id(&self, _: &Arc<SchemaContainer>) -> u32 {
        self.id
    }
}

impl <'a>ToSchemaId for &'a MorpheusSchema {
    fn to_id(&self, _: &Arc<SchemaContainer>) -> u32 {
        self.id
    }
}

impl <'a>ToSchemaId for &'a Schema {
    fn to_id(&self, _: &Arc<SchemaContainer>) -> u32 {
        self.id
    }
}

impl <'a>ToSchemaId for &'a str {
    fn to_id(&self, schemas: &Arc<SchemaContainer>) -> u32 {
        schemas.id_from_name(self).unwrap_or(0)
    }
}

