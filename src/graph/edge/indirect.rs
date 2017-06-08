use neb::ram::schema::Field;
use neb::ram::types::TypeId;

lazy_static! {
    pub static ref EDGE_TEMPLATE: Vec<Field> = vec![
            Field::new(&String::from("_vertex_a"), TypeId::Id as u32, false, false, None),
            Field::new(&String::from("_vertex_b"), TypeId::Id as u32, false, false, None),
        ];
}