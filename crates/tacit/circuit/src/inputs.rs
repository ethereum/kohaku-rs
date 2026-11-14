use ruint::aliases::U256;

/// Depth of the pool's Merkle tree.
pub const TREE_DEPTH: usize = 32;

/// The inputs of the pool's `transact` circuit: two input slots, two output slots.
///
/// The first eleven fields are the public signals, in the order the verifier takes them.
///
/// `Debug` shows only the public signals: the rest is the spender's secret witness.
#[derive(Clone, PartialEq, Eq)]
#[allow(clippy::struct_field_names)]
pub struct CircuitInputs {
    /// A root the pool has held, for the inputs' membership.
    pub root: U256,
    /// The pool's current root, the tree the outputs are inserted into.
    pub old_root: U256,
    /// The root after inserting both outputs.
    pub new_root: U256,
    /// The leaf index of the first output (even).
    pub start_index: U256,
    /// `extAmount − fee mod p`.
    pub public_amount: U256,
    pub ext_data_hash: U256,
    pub asset: U256,
    pub nf: [U256; 2],
    pub out_leaf: [U256; 2],

    pub in_v: [U256; 2],
    pub in_rho: [U256; 2],
    pub in_nk: [U256; 2],
    /// Each input's note spend key `Ak` as `(x, y)`.
    pub in_ak: [[U256; 2]; 2],
    pub in_index: [U256; 2],
    pub in_path: [[U256; TREE_DEPTH]; 2],
    /// Each input's EdDSA-Poseidon signature: `R8` as `(x, y)`, and `S`.
    pub sig_r8: [[U256; 2]; 2],
    pub sig_s: [U256; 2],
    pub out_v: [U256; 2],
    pub out_npk: [U256; 2],
    pub out_rho: [U256; 2],
    /// Siblings from level 1 to the root of the pair slot the outputs fill.
    pub ins_path: [U256; TREE_DEPTH - 1],
}

impl std::fmt::Debug for CircuitInputs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitInputs")
            .field("public_signals", &self.public_signals())
            .finish_non_exhaustive()
    }
}

impl CircuitInputs {
    /// The eleven public signals, in the order the verifier takes them.
    #[must_use]
    pub fn public_signals(&self) -> [U256; 11] {
        [
            self.root,
            self.old_root,
            self.new_root,
            self.start_index,
            self.public_amount,
            self.ext_data_hash,
            self.asset,
            self.nf[0],
            self.nf[1],
            self.out_leaf[0],
            self.out_leaf[1],
        ]
    }

    /// The signals by circuit name, arrays flattened row by row.
    pub(crate) fn signals(&self) -> Vec<(&'static str, Vec<U256>)> {
        let flat2 = |x: &[[U256; 2]; 2]| x.iter().flatten().copied().collect::<Vec<_>>();
        vec![
            ("root", vec![self.root]),
            ("oldRoot", vec![self.old_root]),
            ("newRoot", vec![self.new_root]),
            ("startIndex", vec![self.start_index]),
            ("publicAmount", vec![self.public_amount]),
            ("extDataHash", vec![self.ext_data_hash]),
            ("asset", vec![self.asset]),
            ("nf", self.nf.to_vec()),
            ("outLeaf", self.out_leaf.to_vec()),
            ("inV", self.in_v.to_vec()),
            ("inRho", self.in_rho.to_vec()),
            ("inNk", self.in_nk.to_vec()),
            ("inAk", flat2(&self.in_ak)),
            ("inIndex", self.in_index.to_vec()),
            ("inPath", self.in_path.iter().flatten().copied().collect()),
            ("sigR8", flat2(&self.sig_r8)),
            ("sigS", self.sig_s.to_vec()),
            ("outV", self.out_v.to_vec()),
            ("outNpk", self.out_npk.to_vec()),
            ("outRho", self.out_rho.to_vec()),
            ("insPath", self.ins_path.to_vec()),
        ]
    }
}
