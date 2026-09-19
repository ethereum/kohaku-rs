use ark_ff::Field;
use ark_relations::gr1cs::Matrix;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};

#[derive(Debug, Clone, PartialEq, Eq, CanonicalSerialize, CanonicalDeserialize)]
pub struct SerializableNpIndex<F: Field> {
    pub num_instance_variables: usize,
    pub num_witness_variables: usize,
    pub num_constraints: usize,
    pub a_num_non_zero: usize,
    pub b_num_non_zero: usize,
    pub c_num_non_zero: usize,
    pub a: Matrix<F>,
    pub b: Matrix<F>,
    pub c: Matrix<F>,
}

impl<F: Field> From<ark_circom::index::NPIndex<F>> for SerializableNpIndex<F> {
    fn from(index: ark_circom::index::NPIndex<F>) -> Self {
        Self {
            num_instance_variables: index.num_instance_variables,
            num_witness_variables: index.num_witness_variables,
            num_constraints: index.num_constraints,
            a_num_non_zero: index.a_num_non_zero,
            b_num_non_zero: index.b_num_non_zero,
            c_num_non_zero: index.c_num_non_zero,
            a: index.a,
            b: index.b,
            c: index.c,
        }
    }
}

impl<F: Field> From<SerializableNpIndex<F>> for ark_circom::index::NPIndex<F> {
    fn from(m: SerializableNpIndex<F>) -> Self {
        Self {
            num_instance_variables: m.num_instance_variables,
            num_witness_variables: m.num_witness_variables,
            num_constraints: m.num_constraints,
            a_num_non_zero: m.a_num_non_zero,
            b_num_non_zero: m.b_num_non_zero,
            c_num_non_zero: m.c_num_non_zero,
            a: m.a,
            b: m.b,
            c: m.c,
        }
    }
}
