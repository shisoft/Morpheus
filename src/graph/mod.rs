use neb::ram::schema::{Field, Schema};
use neb::ram::types::{Id, key_hash};
use neb::dovahkiin::types::{Map, Value, ToValue};
use neb::dovahkiin::expr::SExpr;
use neb::ram::cell::{Cell, WriteError, ReadError};
use neb::client::{AsyncClient as NebClient};
use neb::client::transaction::{Transaction, TxnError};
use bifrost::raft::state_machine::master::ExecError;
use bifrost::rpc::RPCError;

use server::schema::{MorpheusSchema, SchemaType, SchemaContainer, SchemaError, ToSchemaId};
use graph::vertex::{Vertex, ToVertexId};
use graph::edge::bilateral::BilateralEdge;
use graph::edge::{EdgeAttributes, EdgeError};
use query::{Tester, Expr, parse_optional_expr};
use futures::prelude::*;
use futures::future;

use std::sync::Arc;

pub mod vertex;
pub mod edge;
pub mod fields;
mod id_list;

#[derive(Debug)]
pub enum NewVertexError {
    SchemaNotFound,
    SchemaNotVertex(SchemaType),
    CannotGenerateCellByData,
    DataNotMap,
    RPCError(RPCError),
    WriteError(WriteError)
}

#[derive(Debug)]
pub enum ReadVertexError {
    RPCError(RPCError),
    ReadError(ReadError),
}

#[derive(Debug)]
pub enum LinkVerticesError {
    EdgeSchemaNotFound,
    SchemaNotEdge,
    BodyRequired,
    BodyShouldNotExisted,
    EdgeError(edge::EdgeError),
}

#[derive(Debug)]
pub enum NeighbourhoodError {
    EdgeError(edge::EdgeError),
    VertexNotFound(Id),
    CannotFindOppositeId(Id),
    FilterEvalError(String)
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum CellType {
    Vertex,
    Edge(edge::EdgeType)
}

#[derive(Clone, Copy)]
pub enum EdgeDirection {
    Inbound,
    Outbound,
    Undirected,
}

impl EdgeDirection {
    pub fn as_field(&self) -> u64 {
        match self {
            &EdgeDirection::Inbound => *fields::INBOUND_KEY_ID,
            &EdgeDirection::Outbound => *fields::OUTBOUND_KEY_ID,
            &EdgeDirection::Undirected => *fields::UNDIRECTED_KEY_ID,
        }
    }
}

fn vertex_to_cell_for_write(schemas: &Arc<SchemaContainer>, vertex: Vertex) -> Result<Cell, NewVertexError> {
    let schema_id = vertex.schema();
    if let Some(stype) = schemas.schema_type(schema_id) {
        if stype != SchemaType::Vertex {
            return Err(NewVertexError::SchemaNotVertex(stype))
        }
    } else {
        return Err(NewVertexError::SchemaNotFound)
    }
    let neb_schema = match schemas.get_neb_schema(schema_id) {
        Some(schema) => schema,
        None => return Err(NewVertexError::SchemaNotFound)
    };
    let mut data = {
        match vertex.cell.data {
            Value::Map(map) => map,
            _ => return Err(NewVertexError::DataNotMap)
        }
    };
    data.insert_key_id(*fields::INBOUND_KEY_ID, Value::Id(Id::unit_id()));
    data.insert_key_id(*fields::OUTBOUND_KEY_ID, Value::Id(Id::unit_id()));
    data.insert_key_id(*fields::UNDIRECTED_KEY_ID, Value::Id(Id::unit_id()));
    match Cell::new(&neb_schema, Value::Map(data)) {
        Some(cell) => Ok(cell),
        None => return Err(NewVertexError::CannotGenerateCellByData)
    }
}

pub struct Graph {
    inner: Arc<GraphInner>
}

pub struct GraphInner {
    schemas: Arc<SchemaContainer>,
    neb_client: Arc<NebClient>
}

impl Graph {
    pub fn new(schemas: &Arc<SchemaContainer>, neb_client: &Arc<NebClient>) -> impl Future<Item = Graph, Error = ExecError> {
        let schemas = schemas.clone();
        let schemas_clone = schemas.clone();
        let neb_client = neb_client.clone();
        GraphInner::check_base_schemas(schemas)
            .and_then(|_| {
                GraphInner::new(schemas_clone, neb_client)
            })
            .and_then(|inner| {
                Ok(Graph { inner: Arc::new(inner) })
            })
    }
    fn check_base_schema(schemas: &Arc<SchemaContainer>, schema_id: u32, schema_name: & 'static str, fields: &'static Field)
        -> impl Future<Item = (), Error = ExecError>
    {
        GraphInner::check_base_schema(schemas.clone(), schema_id, schema_name, fields)
    }
    fn check_base_schemas(schemas: &Arc<SchemaContainer>)
        -> impl Future<Item = (), Error = ExecError>
    {
        GraphInner::check_base_schemas(schemas.clone())
    }
    pub fn new_vertex_group(&self, schema: MorpheusSchema)
        -> impl Future<Item = u32, Error = SchemaError>
    {
        self.inner.new_vertex_group(schema)
    }
    pub fn new_edge_group(&self, schema: MorpheusSchema, edge_attrs: edge::EdgeAttributes)
        -> impl Future<Item = u32, Error = SchemaError>
    {
        self.inner.new_edge_group(schema, edge_attrs)
    }
    pub fn new_vertex<S>(&self, schema: S, data: Map)
        -> impl Future<Item = Vertex, Error = NewVertexError>
        where S: ToSchemaId
    {
        GraphInner::new_vertex(self.inner.clone(), schema, data)
    }
    pub fn remove_vertex<V>(&self, vertex: V)
        -> impl Future<Item = (), Error = TxnError> where V: ToVertexId
    {
        self.inner.remove_vertex(vertex)
    }
    pub fn remove_vertex_by_key<K, S>(&self, schema: S, key: K)
        -> impl Future<Item = (), Error = TxnError>
        where K: ToValue, S: ToSchemaId
    {
        self.inner.remove_vertex_by_key(schema, key)
    }
    pub fn update_vertex<V, U>(&self, vertex: V, update: U)
        -> impl Future<Item = (), Error = TxnError>
        where V: ToVertexId, U: Fn(Vertex) -> Option<Vertex>, U: 'static
    {
        self.inner.update_vertex(vertex, update)
    }
    pub fn update_vertex_by_key<K, U, S>(&self, schema: S, key: K, update: U)
        -> impl Future<Item = (), Error = TxnError>
        where K: ToValue, S: ToSchemaId, U: Fn(Vertex) -> Option<Vertex>, U: 'static
    {
        self.inner.update_vertex_by_key(schema, key, update)
    }

    pub fn vertex_by<V>(&self, vertex: V)
        -> impl Future<Item = Option<Vertex>, Error = ReadVertexError>
        where V: ToVertexId
    {
        GraphInner::vertex_by(self.inner.clone(), vertex)
    }

    pub fn vertex_by_key<K, S>(&self, schema: S, key: K)
        -> impl Future<Item = Option<Vertex>, Error = ReadVertexError>
        where K: ToValue, S: ToSchemaId
    {
        GraphInner::vertex_by_key(self.inner.clone(), schema, key)
    }

    pub fn graph_transaction<TFN, TR>(&self, func: TFN)
        -> impl Future<Item = TR, Error = TxnError>
        where TFN: Fn(&GraphTransaction) -> Result<TR, TxnError>, TR: 'static, TFN: 'static
    {
        self.inner.graph_transaction(func)
    }
    pub fn link<V, S>(&self, from: V, schema: S, to: V, body: Option<Map>)
        -> impl Future<Item = Result<edge::Edge, LinkVerticesError>, Error = TxnError>
        where V: ToVertexId, S: ToSchemaId
    {
        self.inner.link(from, schema, to, body)
    }
    pub fn degree<V, S>(&self, vertex: V, schema: S, direction: EdgeDirection)
        -> impl Future<Item = Result<usize, edge::EdgeError>, Error = TxnError>
        where V: ToVertexId, S: ToSchemaId
    {
        self.inner.degree(vertex, schema, direction)
    }
    pub fn neighbourhoods<V, S, F>(&self, vertex: V, schema: S, direction: EdgeDirection, filter: &Option<F>)
        -> impl Future<Item = Result<Vec<(Vertex, edge::Edge)>, NeighbourhoodError>, Error = TxnError>
        where V: ToVertexId, S: ToSchemaId, F: Expr
    {
        GraphInner::neighbourhoods(self.inner.clone(), vertex, schema, direction, filter)
    }
    pub fn edges<V, S, F>(&self, vertex: V, schema: S, direction: EdgeDirection, filter: &Option<F>)
        -> impl Future<Item = Result<Vec<edge::Edge>, EdgeError>, Error = TxnError>
        where V: ToVertexId, S: ToSchemaId, F: Expr
    {
        GraphInner::edges(self.inner.clone(), vertex, schema, direction, filter)
    }
}

impl GraphInner {
    #[async]
    pub fn new(schemas: Arc<SchemaContainer>, neb_client: Arc<NebClient>) -> Result<GraphInner, ExecError> {
        await!(GraphInner::check_base_schemas(schemas.clone()))?;
        Ok(GraphInner {
            schemas: schemas.clone(),
            neb_client: neb_client.clone()
        })
    }
    #[async]
    fn check_base_schema(schemas: Arc<SchemaContainer>, schema_id: u32, schema_name: &'static str, fields: &'static Field) -> Result<(), ExecError> {
        match schemas.get_neb_schema(schema_id) {
            None => {
                await!(schemas.neb_client.new_schema_with_id(
                    Schema::new_with_id(
                        schema_id, schema_name, None, fields.clone(), false
                    )
                ))?;
            },
            _ => {}
        }
        Ok(())
    }
    #[async]
    fn check_base_schemas(schemas: Arc<SchemaContainer>) -> Result<(), ExecError> {
        await!(GraphInner::check_base_schema(schemas.clone(), id_list::ID_LIST_SCHEMA_ID, "_NEB_ID_LIST", &*id_list::ID_LINKED_LIST))?;
        await!(GraphInner::check_base_schema(schemas, id_list::TYPE_LIST_SCHEMA_ID, "_NEB_TYPE_ID_LIST", &*id_list::ID_TYPE_LIST))?;
        Ok(())
    }
    pub fn new_vertex_group(&self, mut schema: MorpheusSchema)
        -> impl Future<Item = u32, Error = SchemaError>
    {
        schema.schema_type = SchemaType::Vertex;
        self.schemas.new_schema(schema)
    }
    pub fn new_edge_group(&self, mut schema: MorpheusSchema, edge_attrs: edge::EdgeAttributes)
        -> impl Future<Item = u32, Error = SchemaError>
    {
        schema.schema_type = SchemaType::Edge(edge_attrs);
        self.schemas.new_schema(schema)
    }
    pub fn new_vertex<S>(this: Arc<Self>, schema: S, data: Map)
        -> impl Future<Item = Vertex, Error = NewVertexError>
        where S: ToSchemaId
    {
        let vertex = Vertex::new(schema.to_id(&this.schemas), data);
        let mut cell_result = vertex_to_cell_for_write(&this.schemas, vertex);
        async_block! {
            let mut cell = cell_result?;
            let header = match await!(this.neb_client.write_cell(cell.clone())) {
                Ok(Ok(header)) => header,
                Ok(Err(e)) => return Err(NewVertexError::WriteError(e)),
                Err(e) => return Err(NewVertexError::RPCError(e))
            };
            cell.header = header;
            Ok(vertex::cell_to_vertex(cell))
        }
    }
    pub fn remove_vertex<V>(&self, vertex: V)
        -> impl Future<Item = (), Error = TxnError> where V: ToVertexId
    {
        let id = vertex.to_id();
        self.graph_transaction(move |txn| txn.remove_vertex(id)?
            .map_err(|_| TxnError::Aborted(None)))
    }
    pub fn remove_vertex_by_key<K, S>(&self, schema: S, key: K)
        -> impl Future<Item = (), Error = TxnError>
        where K: ToValue, S: ToSchemaId
    {
        let id = Cell::encode_cell_key(schema.to_id(&self.schemas), &key.value());
        self.remove_vertex(id)
    }
    pub fn update_vertex<V, U>(&self, vertex: V, update: U) -> impl Future<Item = (), Error = TxnError>
        where V: ToVertexId, U: Fn(Vertex) -> Option<Vertex>, U: 'static
    {
        let id = vertex.to_id();
        self.neb_client.transaction(move |txn|{
            vertex::txn_update(txn, id, &update)
        })
    }
    pub fn update_vertex_by_key<K, U, S>(&self, schema: S, key: K, update: U)
        -> impl Future<Item = (), Error = TxnError>
        where K: ToValue, S: ToSchemaId, U: Fn(Vertex) -> Option<Vertex>, U: 'static
    {
        let id = Cell::encode_cell_key(schema.to_id(&self.schemas), &key.value());
        self.update_vertex(id, update)
    }

    pub fn vertex_by<V>(this: Arc<Self>, vertex: V)
        -> impl Future<Item = Option<Vertex>, Error = ReadVertexError> where V: ToVertexId
    {
        this.neb_client.read_cell(vertex.to_id())
            .then(|result| {
                match result {
                    Err(e) => Err(ReadVertexError::RPCError(e)),
                    Ok(Err(ReadError::CellDoesNotExisted)) => Ok(None),
                    Ok(Err(e)) => Err(ReadVertexError::ReadError(e)),
                    Ok(Ok(cell)) => Ok(Some(vertex::cell_to_vertex(cell)))
                }
            })
    }

    pub fn vertex_by_key<K, S>(this: Arc<Self>, schema: S, key: K)
        -> impl Future<Item = Option<Vertex>, Error = ReadVertexError>
        where K: ToValue, S: ToSchemaId
    {
        let id = Cell::encode_cell_key(schema.to_id(&this.schemas), &key.value());
        Self::vertex_by(this, id)
    }

    pub fn graph_transaction<TFN, TR>(&self, func: TFN) -> impl Future<Item = TR, Error = TxnError>
        where TFN: Fn(&GraphTransaction) -> Result<TR, TxnError>, TR: 'static, TFN: 'static
    {
        let schemas = self.schemas.clone();
        let wrapper = move |neb_txn: &Transaction| {
            func(&GraphTransaction {
                neb_txn,
                schemas: schemas.clone()
            })
        };
        self.neb_client.transaction(wrapper)
    }
    pub fn link<V, S>(&self, from: V, schema: S, to: V, body: Option<Map>)
        -> impl Future<Item = Result<edge::Edge, LinkVerticesError>, Error = TxnError>
        where V: ToVertexId, S: ToSchemaId
    {
        let from_id = from.to_id();
        let to_id = to.to_id();
        let schema_id = schema.to_id(&self.schemas);
        self.graph_transaction(move |txn| {
            txn.link(from_id, schema_id, to_id, body.clone())
        })
    }
    pub fn degree<V, S>(&self, vertex: V, schema: S, ed: EdgeDirection)
        -> impl Future<Item = Result<usize, edge::EdgeError>, Error = TxnError>
        where V: ToVertexId, S: ToSchemaId
    {
        let vertex_id = vertex.to_id();
        let schema_id = schema.to_id(&self.schemas);
        self.graph_transaction(move |txn| {
            txn.degree(vertex_id, schema_id, ed)
        })
    }
    pub fn neighbourhoods<V, S, F>(this: Arc<Self>, vertex: V, schema: S, ed: EdgeDirection, filter: &Option<F>)
        -> impl Future<Item = Result<Vec<(Vertex, edge::Edge)>, NeighbourhoodError>, Error = TxnError>
        where V: ToVertexId, S: ToSchemaId, F: Expr
    {
        let vertex_id = vertex.to_id();
        let schema_id = schema.to_id(&this.schemas);
        future::result(parse_optional_expr(filter))
            .map_err(|e| {
                NeighbourhoodError::FilterEvalError(e)
            })
            .then(move |filter_sexpr_result| {
                async_block! {
                    match filter_sexpr_result {
                        Ok(filter_sexpr) => {
                            return await!(this.graph_transaction(move |txn| {
                                txn.neighbourhoods(vertex_id, schema_id, ed, &filter_sexpr)
                            }))
                        },
                        Err(e) => return Ok(Err(e))
                    }
                }
            })
    }
    pub fn edges<V, S, F>(this: Arc<Self>, vertex: V, schema: S, ed: EdgeDirection, filter: &Option<F>)
        -> impl Future<Item = Result<Vec<edge::Edge>, EdgeError>, Error = TxnError>
        where V: ToVertexId, S: ToSchemaId, F: Expr
    {
        let vertex_id = vertex.to_id();
        let schema_id = schema.to_id(&this.schemas);
        future::result(parse_optional_expr(filter))
            .map_err(|e| {
                EdgeError::FilterEvalError(e)
            })
            .then(move |filter_result| {
                async_block! {
                    match filter_result {
                        Ok(filter) => {
                            return await!(this.graph_transaction(move |txn| {
                                txn.edges(vertex_id, schema_id, ed, &filter)
                            }))
                        },
                        Err(e) => return Ok(Err(e))
                    }
                }
            })
    }
}

pub struct GraphTransaction<'a> {
    pub neb_txn: &'a Transaction,
    schemas: Arc<SchemaContainer>
}

impl <'a>GraphTransaction<'a> {
    pub fn new_vertex<S>(&self, schema: S, data: Map)
        -> Result<Result<Vertex, NewVertexError>, TxnError>
        where S: ToSchemaId
    {
        let vertex = Vertex::new(schema.to_id(&self.schemas), data);
        let mut cell = match vertex_to_cell_for_write(&self.schemas, vertex) {
            Ok(cell) => cell, Err(e) => return Ok(Err(e))
        };
        self.neb_txn.write(&cell)?;
        Ok(Ok(vertex::cell_to_vertex(cell)))
    }
    pub fn remove_vertex<V>(&self, vertex: V)
        -> Result<Result<(), vertex::RemoveError>, TxnError> where V: ToVertexId
    {
        vertex::txn_remove(self.neb_txn, &self.schemas, vertex)
    }
    pub fn remove_vertex_by_key<K, S>(&self, schema: S, key: K)
        -> Result<Result<(), vertex::RemoveError>, TxnError>
        where K: ToValue, S: ToSchemaId
    {
        let id = Cell::encode_cell_key(schema.to_id(&self.schemas), &key.value());
        self.remove_vertex(&id)
    }

    pub fn link<V, S>(&self, from: V, schema: S, to: V, body: Option<Map>)
        -> Result<Result<edge::Edge, LinkVerticesError>, TxnError>
        where V: ToVertexId, S: ToSchemaId
    {
        let from_id = &from.to_id();
        let to_id = &to.to_id();
        let schema_id = schema.to_id(&self.schemas);
        let edge_attr = match self.schemas.schema_type(schema_id) {
            Some(SchemaType::Edge(ea)) => ea,
            Some(_) => return Ok(Err(LinkVerticesError::SchemaNotEdge)),
            None => return Ok(Err(LinkVerticesError::EdgeSchemaNotFound))
        };
        match edge_attr.edge_type {
            edge::EdgeType::Directed =>
                Ok(edge::directed::DirectedEdge::link(from_id, to_id, body, &self.neb_txn, schema_id, &self.schemas)?
                    .map_err(LinkVerticesError::EdgeError).map(edge::Edge::Directed)),

            edge::EdgeType::Undirected =>
                Ok(edge::undirectd::UndirectedEdge::link(from_id, to_id, body, &self.neb_txn, schema_id, &self.schemas)?
                    .map_err(LinkVerticesError::EdgeError).map(edge::Edge::Undirected))
        }
    }

    pub fn update_vertex<V, U>(&self, vertex: V, update: U) -> Result<(), TxnError>
        where V: ToVertexId, U: Fn(Vertex) -> Option<Vertex>
    {
        vertex::txn_update(self.neb_txn, vertex, &update)
    }
    pub fn update_vertex_by_key<K, U, S>(&self, schema: S, key: K, update: U)
        -> Result<(), TxnError>
        where K: ToValue, S: ToSchemaId, U: Fn(Vertex) -> Option<Vertex>
    {
        let id = Cell::encode_cell_key(schema.to_id(&self.schemas), &key.value());
        self.update_vertex(&id, update)
    }

    pub fn read_vertex<V>(&self, vertex: V)
        -> Result<Option<Vertex>, TxnError> where V: ToVertexId
    {
        self.neb_txn.read(&vertex.to_id()).map(|c| c.map(vertex::cell_to_vertex))
    }

    pub fn get_vertex<K, S>(&self, schema: u32, key: K) -> Result<Option<Vertex>, TxnError>
        where K: ToValue, S: ToSchemaId
    {
        let id = Cell::encode_cell_key(schema.to_id(&self.schemas), &key.value());
        self.read_vertex(&id)
    }

    pub fn edges<V, S>(
        &self, vertex: V, schema: S, ed: EdgeDirection, filter: &Option<Vec<SExpr>>
    ) -> Result<Result<Vec<edge::Edge>, edge::EdgeError>, TxnError>
        where V: ToVertexId, S: ToSchemaId
    {
        let vertex_field = ed.as_field();
        let schema_id = schema.to_id(&self.schemas);
        let vertex_id = &vertex.to_id();
        match id_list::IdList::from_txn_and_container
            (self.neb_txn, vertex_id, vertex_field, schema_id).iter()? {
            Err(e) => Ok(Err(edge::EdgeError::IdListError(e))),
            Ok(ids) => Ok(Ok({
                let mut edges = Vec::new();
                for id in ids {
                    match edge::from_id(
                        vertex_id, vertex_field, schema_id, &self.schemas, self.neb_txn, &id
                    )? {
                        Ok(e) => {
                            match Tester::eval_with_edge(filter, &e) {
                                Ok(true) => {edges.push(e);},
                                Ok(false) => {},
                                Err(err) => return Ok(Err(EdgeError::FilterEvalError(err))),
                            }
                        },
                        Err(er) => return Ok(Err(er))
                    }
                }
                edges
            }))
        }
    }

    pub fn neighbourhoods<V, S>(
        &self, vertex: V, schema: S, ed: EdgeDirection, filter: &Option<Vec<SExpr>>
    )
        -> Result<Result<Vec<(Vertex, edge::Edge)>, NeighbourhoodError>, TxnError>
        where V: ToVertexId, S: ToSchemaId
    {
        let vertex_field = ed.as_field();
        let schema_id = schema.to_id(&self.schemas);
        let vertex_id = &vertex.to_id();
        match id_list::IdList::from_txn_and_container
            (self.neb_txn, vertex_id, vertex_field, schema_id).iter()? {
            Err(e) => Ok(Err(NeighbourhoodError::EdgeError(EdgeError::IdListError(e)))),
            Ok(ids) => {
                let mut result: Vec<(Vertex, edge::Edge)> = Vec::new();
                for id in ids {
                    match edge::from_id(
                        vertex_id, vertex_field, schema_id, &self.schemas, self.neb_txn, &id
                    )? {
                        Ok(edge) => {
                            let vertex = if let Some(opposite_id) = edge.one_opposite_id_vertex_id(vertex_id) {
                                if let Some(v) = self.read_vertex(opposite_id)? { v } else {
                                    return Ok(Err(NeighbourhoodError::VertexNotFound(*opposite_id)))
                                }
                            } else { return Ok(Err(NeighbourhoodError::CannotFindOppositeId(*vertex_id))) };
                            match Tester::eval_with_edge_and_vertex(filter, &vertex, &edge) {
                                Ok(true) => {result.push((vertex, edge));},
                                Ok(false) => {},
                                Err(err) => return Ok(Err(NeighbourhoodError::FilterEvalError(err))),
                            }
                        },
                        Err(edge_error) => return Ok(Err(NeighbourhoodError::EdgeError(edge_error)))
                    }
                }
                return Ok(Ok(result));
            }
        }
    }

    pub fn degree<V, S>(&self, vertex: V, schema: S, ed: EdgeDirection)
        -> Result<Result<usize, edge::EdgeError>, TxnError>
        where V: ToVertexId, S: ToSchemaId
    {
        let (schema_id, edge_attr) = match edge_attr_from_schema(schema, &self.schemas) {
            Err(e) => return Ok(Err(e)), Ok(t) => t
        };
        let vertex_field = ed.as_field();
        let vertex_id = &vertex.to_id();
        match id_list::IdList::from_txn_and_container
            (self.neb_txn, vertex_id, vertex_field, schema_id).count()? {
            Err(e) => Ok(Err(edge::EdgeError::IdListError(e))),
            Ok(count) => Ok(Ok(count))
        }
    }
}

pub fn edge_attr_from_schema<S>(schema: S, schemas: &Arc<SchemaContainer>)
    -> Result<(u32, EdgeAttributes), EdgeError>
    where S: ToSchemaId
{
    let schema_id = schema.to_id(schemas);
    Ok((
        schema_id,
        match schemas.schema_type(schema_id) {
            Some(SchemaType::Edge(ea)) => ea,
            Some(_) => return Err(EdgeError::WrongSchema),
            None => return Err(EdgeError::CannotFindSchema)
        }
    ))
}
