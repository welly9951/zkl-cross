use self::allocator::*;
use self::constraint_builder::ConstraintBuilder;
use super::bit_table::BitTableConfig;
use super::brtable::BrTableConfig;
use super::cell::*;
use super::config::max_etable_rows;
use super::external_host_call_table::ExternalHostCallTableConfig;
use super::itable::InstructionTableConfig;
use super::jtable::JumpTableConfig;
use super::mtable::MemoryTableConfig;
use super::rtable::RangeTableConfig;
use super::traits::ConfigureLookupTable;
use super::utils::step_status::StepStatus;
use super::utils::table_entry::EventTableEntryWithMemoryInfo;
use super::utils::Context;
use super::CircuitConfigure;
use super::Lookup;
use crate::circuits::etable::op_configure::op_bin::BinConfigBuilder;
use crate::circuits::etable::op_configure::op_bin_bit::BinBitConfigBuilder;
use crate::circuits::etable::op_configure::op_bin_shift::BinShiftConfigBuilder;
use crate::circuits::etable::op_configure::op_br::BrConfigBuilder;
use crate::circuits::etable::op_configure::op_br_if::BrIfConfigBuilder;
use crate::circuits::etable::op_configure::op_br_if_eqz::BrIfEqzConfigBuilder;
use crate::circuits::etable::op_configure::op_br_table::BrTableConfigBuilder;
use crate::circuits::etable::op_configure::op_call::CallConfigBuilder;
use crate::circuits::etable::op_configure::op_call_host_foreign_circuit::ExternalCallHostCircuitConfigBuilder;
use crate::circuits::etable::op_configure::op_call_indirect::CallIndirectConfigBuilder;
use crate::circuits::etable::op_configure::op_const::ConstConfigBuilder;
use crate::circuits::etable::op_configure::op_conversion::ConversionConfigBuilder;
use crate::circuits::etable::op_configure::op_drop::DropConfigBuilder;
use crate::circuits::etable::op_configure::op_global_get::GlobalGetConfigBuilder;
use crate::circuits::etable::op_configure::op_global_set::GlobalSetConfigBuilder;
use crate::circuits::etable::op_configure::op_load::LoadConfigBuilder;
use crate::circuits::etable::op_configure::op_local_get::LocalGetConfigBuilder;
use crate::circuits::etable::op_configure::op_local_set::LocalSetConfigBuilder;
use crate::circuits::etable::op_configure::op_local_tee::LocalTeeConfigBuilder;
use crate::circuits::etable::op_configure::op_memory_grow::MemoryGrowConfigBuilder;
use crate::circuits::etable::op_configure::op_memory_size::MemorySizeConfigBuilder;
use crate::circuits::etable::op_configure::op_rel::RelConfigBuilder;
use crate::circuits::etable::op_configure::op_return::ReturnConfigBuilder;
use crate::circuits::etable::op_configure::op_select::SelectConfigBuilder;
use crate::circuits::etable::op_configure::op_store::StoreConfigBuilder;
use crate::circuits::etable::op_configure::op_test::TestConfigBuilder;
use crate::circuits::etable::op_configure::op_unary::UnaryConfigBuilder;
use crate::constant_from;
use crate::fixed_curr;
use crate::foreign::require_helper::etable_op_configure::ETableRequireHelperTableConfigBuilder;
use crate::foreign::wasm_input_helper::etable_op_configure::ETableWasmInputHelperTableConfigBuilder;
use crate::foreign::EventTableForeignCallConfigBuilder;
use crate::foreign::ForeignTableConfig;
use crate::foreign::InternalHostPluginBuilder;
use halo2_proofs::arithmetic::FieldExt;
use halo2_proofs::plonk::Advice;
use halo2_proofs::plonk::Column;
use halo2_proofs::plonk::ConstraintSystem;
use halo2_proofs::plonk::Error;
use halo2_proofs::plonk::Expression;
use halo2_proofs::plonk::Fixed;
use halo2_proofs::plonk::VirtualCells;
use specs::encode::instruction_table::encode_instruction_table_entry;
use specs::etable::EventTableEntry;
use specs::itable::OpcodeClass;
use specs::itable::OpcodeClassPlain;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::rc::Rc;

mod assign;
mod op_configure;

pub(crate) mod allocator;
pub(crate) mod constraint_builder;

pub(crate) const EVENT_TABLE_ENTRY_ROWS: i32 = 4;
pub(crate) const OP_LVL1_BITS: usize = 6;
pub(crate) const OP_LVL2_BITS: usize = 6;

#[derive(Clone)]
pub struct EventTableCommonConfig<F: FieldExt> {
    enabled_cell: AllocatedBitCell<F>,

    lvl1_bits: [AllocatedBitCell<F>; 6],
    lvl2_bits: [AllocatedBitCell<F>; 6],

    rest_mops_cell: AllocatedCommonRangeCell<F>,
    rest_jops_cell: AllocatedCommonRangeCell<F>,
    pub(crate) input_index_cell: AllocatedCommonRangeCell<F>,
    external_host_call_index_cell: AllocatedCommonRangeCell<F>,
    pub(crate) sp_cell: AllocatedCommonRangeCell<F>,
    mpages_cell: AllocatedCommonRangeCell<F>,
    frame_id_cell: AllocatedCommonRangeCell<F>,
    pub(crate) eid_cell: AllocatedCommonRangeCell<F>,
    fid_cell: AllocatedCommonRangeCell<F>,
    iid_cell: AllocatedCommonRangeCell<F>,

    itable_lookup_cell: AllocatedUnlimitedCell<F>,
    brtable_lookup_cell: AllocatedUnlimitedCell<F>,
    jtable_lookup_cell: AllocatedUnlimitedCell<F>,
    pow_table_lookup_cell: AllocatedUnlimitedCell<F>,
    bit_table_lookup_cell: AllocatedUnlimitedCell<F>,
    external_foreign_call_lookup_cell: AllocatedUnlimitedCell<F>,

    circuit_configure: CircuitConfigure,
}

impl<F: FieldExt> EventTableCommonConfig<F> {
    pub(self) fn allocate_opcode_bit_cell(
        &self,
        opcode_class_plain: OpcodeClassPlain,
    ) -> (AllocatedBitCell<F>, AllocatedBitCell<F>) {
        let (lvl1, lvl2) = Self::opclass_to_two_level(opcode_class_plain);

        (
            *self.lvl1_bits.get(lvl1).unwrap(),
            *self.lvl2_bits.get(lvl2).unwrap(),
        )
    }

    fn opclass_to_two_level(opcode_class_plain: OpcodeClassPlain) -> (usize, usize) {
        // OpcodeClassPlain starts from 1.
        let idx = opcode_class_plain.0 - 1;

        assert!(idx < OP_LVL1_BITS * OP_LVL2_BITS);
        (idx / OP_LVL2_BITS, idx % OP_LVL2_BITS)
    }
}

pub(in crate::circuits::etable) trait EventTableOpcodeConfigBuilder<F: FieldExt> {
    fn configure(
        common: &EventTableCommonConfig<F>,
        allocator: &mut EventTableCellAllocator<F>,
        constraint_builder: &mut ConstraintBuilder<F>,
    ) -> Box<dyn EventTableOpcodeConfig<F>>;
}

pub trait EventTableOpcodeConfig<F: FieldExt> {
    fn opcode(&self, meta: &mut VirtualCells<'_, F>) -> Expression<F>;
    fn assign(
        &self,
        ctx: &mut Context<'_, F>,
        step: &StepStatus,
        entry: &EventTableEntryWithMemoryInfo,
    ) -> Result<(), Error>;
    fn memory_writing_ops(&self, _: &EventTableEntry) -> u32 {
        0
    }
    fn sp_diff(&self, _meta: &mut VirtualCells<'_, F>) -> Option<Expression<F>> {
        None
    }
    fn allocated_memory_pages_diff(
        &self,
        _meta: &mut VirtualCells<'_, F>,
    ) -> Option<Expression<F>> {
        None
    }
    fn jops_expr(&self, _meta: &mut VirtualCells<'_, F>) -> Option<Expression<F>> {
        None
    }
    fn jops(&self) -> u32 {
        0
    }
    fn mops(&self, _meta: &mut VirtualCells<'_, F>) -> Option<Expression<F>> {
        None
    }
    fn next_frame_id(
        &self,
        _meta: &mut VirtualCells<'_, F>,
        _common_config: &EventTableCommonConfig<F>,
    ) -> Option<Expression<F>> {
        None
    }
    fn next_fid(
        &self,
        _meta: &mut VirtualCells<'_, F>,
        _common_config: &EventTableCommonConfig<F>,
    ) -> Option<Expression<F>> {
        None
    }
    fn next_iid(
        &self,
        _meta: &mut VirtualCells<'_, F>,
        _common_config: &EventTableCommonConfig<F>,
    ) -> Option<Expression<F>> {
        None
    }
    fn input_index_increase(
        &self,
        _meta: &mut VirtualCells<'_, F>,
        _common_config: &EventTableCommonConfig<F>,
    ) -> Option<Expression<F>> {
        None
    }
    fn is_host_public_input(&self, _entry: &EventTableEntry) -> bool {
        false
    }
    fn external_host_call_index_increase(
        &self,
        _meta: &mut VirtualCells<'_, F>,
        _common_config: &EventTableCommonConfig<F>,
    ) -> Option<Expression<F>> {
        None
    }
    fn is_external_host_call(&self, _entry: &EventTableEntry) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct EventTableConfig<F: FieldExt> {
    pub step_sel: Column<Fixed>,
    pub common_config: EventTableCommonConfig<F>,
    op_configs: BTreeMap<OpcodeClassPlain, Rc<Box<dyn EventTableOpcodeConfig<F>>>>,
}

impl<F: FieldExt> EventTableConfig<F> {
    pub(crate) fn configure(
        meta: &mut ConstraintSystem<F>,
        cols: &mut (impl Iterator<Item = Column<Advice>> + Clone),
        circuit_configure: &CircuitConfigure,
        rtable: &RangeTableConfig<F>,
        itable: &InstructionTableConfig<F>,
        mtable: &MemoryTableConfig<F>,
        jtable: &JumpTableConfig<F>,
        brtable: &BrTableConfig<F>,
        bit_table: &BitTableConfig<F>,
        external_host_call_table: &ExternalHostCallTableConfig<F>,
        foreign_table_configs: &BTreeMap<&'static str, Box<dyn ForeignTableConfig<F>>>,
        opcode_set: &HashSet<OpcodeClassPlain>,
    ) -> EventTableConfig<F> {
        let step_sel = meta.fixed_column();

        let mut allocator = EventTableCellAllocator::new(meta, step_sel, rtable, mtable, cols);
        allocator.enable_equality(meta, &EventTableCellType::CommonRange);

        let lvl1_bits = [0; OP_LVL1_BITS].map(|_| allocator.alloc_bit_cell());
        let lvl2_bits = [0; OP_LVL2_BITS].map(|_| allocator.alloc_bit_cell());

        let enabled_cell = allocator.alloc_bit_cell();
        let rest_mops_cell = allocator.alloc_common_range_cell();
        let rest_jops_cell = allocator.alloc_common_range_cell();
        let input_index_cell = allocator.alloc_common_range_cell();
        let external_host_call_index_cell = allocator.alloc_common_range_cell();
        let sp_cell = allocator.alloc_common_range_cell();
        let mpages_cell = allocator.alloc_common_range_cell();
        let frame_id_cell = allocator.alloc_common_range_cell();
        let eid_cell = allocator.alloc_common_range_cell();
        let fid_cell = allocator.alloc_common_range_cell();
        let iid_cell = allocator.alloc_common_range_cell();

        let itable_lookup_cell = allocator.alloc_unlimited_cell();
        let brtable_lookup_cell = allocator.alloc_unlimited_cell();
        let jtable_lookup_cell = allocator.alloc_unlimited_cell();
        let pow_table_lookup_cell = allocator.alloc_unlimited_cell();
        let bit_table_lookup_cell = allocator.alloc_unlimited_cell();
        let external_foreign_call_lookup_cell = allocator.alloc_unlimited_cell();

        let common_config = EventTableCommonConfig {
            enabled_cell,
            lvl1_bits,
            lvl2_bits,
            rest_mops_cell,
            rest_jops_cell,
            input_index_cell,
            external_host_call_index_cell,
            sp_cell,
            mpages_cell,
            frame_id_cell,
            eid_cell,
            fid_cell,
            iid_cell,
            itable_lookup_cell,
            brtable_lookup_cell,
            jtable_lookup_cell,
            pow_table_lookup_cell,
            bit_table_lookup_cell,
            external_foreign_call_lookup_cell,
            circuit_configure: circuit_configure.clone(),
        };

        let mut op_bitmaps: BTreeMap<OpcodeClassPlain, (usize, usize)> = BTreeMap::new();
        let mut op_configs: BTreeMap<OpcodeClassPlain, Rc<Box<dyn EventTableOpcodeConfig<F>>>> =
            BTreeMap::new();

        #[cfg(feature="checksum")]
        const OPTIMIZE_GATES: bool = false;
        #[cfg(not(feature="checksum"))]
        const OPTIMIZE_GATES: bool = true;

        macro_rules! configure {
            ($op:expr, $x:ident) => {
                let op = OpcodeClassPlain($op as usize);

                if !OPTIMIZE_GATES || opcode_set.contains(&op) {
                    let (op_lvl1, op_lvl2) = EventTableCommonConfig::<F>::opclass_to_two_level(op);
                    let foreign_table_configs = BTreeMap::new();
                    let mut constraint_builder =
                        ConstraintBuilder::new(meta, &foreign_table_configs);

                    let config = $x::configure(
                        &common_config,
                        &mut allocator.clone(),
                        &mut constraint_builder,
                    );

                    constraint_builder.finalize(|meta| {
                        fixed_curr!(meta, step_sel)
                            * lvl1_bits[op_lvl1].curr_expr(meta)
                            * lvl2_bits[op_lvl2].curr_expr(meta)
                    });

                    op_bitmaps.insert(op, (op_lvl1, op_lvl2));
                    op_configs.insert(op, Rc::new(config));
                }
            };
        }

        configure!(OpcodeClass::BinShift, BinShiftConfigBuilder);
        configure!(OpcodeClass::Bin, BinConfigBuilder);
        configure!(OpcodeClass::BrIfEqz, BrIfEqzConfigBuilder);
        configure!(OpcodeClass::BrIf, BrIfConfigBuilder);
        configure!(OpcodeClass::Br, BrConfigBuilder);
        configure!(OpcodeClass::Call, CallConfigBuilder);
        configure!(OpcodeClass::CallHost, ExternalCallHostCircuitConfigBuilder);
        configure!(OpcodeClass::Const, ConstConfigBuilder);
        configure!(OpcodeClass::Conversion, ConversionConfigBuilder);
        configure!(OpcodeClass::Drop, DropConfigBuilder);
        configure!(OpcodeClass::GlobalGet, GlobalGetConfigBuilder);
        configure!(OpcodeClass::GlobalSet, GlobalSetConfigBuilder);
        configure!(OpcodeClass::LocalGet, LocalGetConfigBuilder);
        configure!(OpcodeClass::LocalSet, LocalSetConfigBuilder);
        configure!(OpcodeClass::LocalTee, LocalTeeConfigBuilder);
        configure!(OpcodeClass::Rel, RelConfigBuilder);
        configure!(OpcodeClass::Return, ReturnConfigBuilder);
        configure!(OpcodeClass::Select, SelectConfigBuilder);
        configure!(OpcodeClass::Test, TestConfigBuilder);
        configure!(OpcodeClass::Unary, UnaryConfigBuilder);
        configure!(OpcodeClass::Load, LoadConfigBuilder);
        configure!(OpcodeClass::Store, StoreConfigBuilder);
        configure!(OpcodeClass::BinBit, BinBitConfigBuilder);
        configure!(OpcodeClass::MemorySize, MemorySizeConfigBuilder);
        configure!(OpcodeClass::MemoryGrow, MemoryGrowConfigBuilder);
        configure!(OpcodeClass::BrTable, BrTableConfigBuilder);
        configure!(OpcodeClass::CallIndirect, CallIndirectConfigBuilder);

        let mut plugin_index = 0;
        macro_rules! configure_foreign {
            ($x:ident, $i:expr) => {
                let builder = $x::new($i);
                let op = OpcodeClass::ForeignPluginStart as usize + $i;
                let op = OpcodeClassPlain(op);

                if !OPTIMIZE_GATES || opcode_set.contains(&op) {
                    let (op_lvl1, op_lvl2) = EventTableCommonConfig::<F>::opclass_to_two_level(op);
                    let mut constraint_builder =
                        ConstraintBuilder::new(meta, foreign_table_configs);

                    let config = builder.configure(
                        &common_config,
                        &mut allocator.clone(),
                        &mut constraint_builder,
                    );

                    constraint_builder.finalize(|meta| {
                        fixed_curr!(meta, step_sel)
                            * lvl1_bits[op_lvl1].curr_expr(meta)
                            * lvl2_bits[op_lvl2].curr_expr(meta)
                    });

                    op_bitmaps.insert(op, (op_lvl1, op_lvl2));
                    op_configs.insert(op, Rc::new(config));
                }

                plugin_index += 1;
            };
        }
        configure_foreign!(ETableWasmInputHelperTableConfigBuilder, 0);
        configure_foreign!(ETableRequireHelperTableConfigBuilder, 2);
        drop(plugin_index);

        meta.create_gate("c1. enable seq", |meta| {
            vec![
                enabled_cell.next_expr(meta)
                    * (enabled_cell.curr_expr(meta) - constant_from!(1))
                    * fixed_curr!(meta, step_sel),
            ]
        });

        meta.create_gate("c4. opcode_bit lvl sum equals to 1", |meta| {
            vec![
                lvl1_bits
                    .map(|x| x.curr_expr(meta))
                    .into_iter()
                    .reduce(|acc, x| acc + x)
                    .unwrap()
                    - constant_from!(1),
                lvl2_bits
                    .map(|x| x.curr_expr(meta))
                    .into_iter()
                    .reduce(|acc, x| acc + x)
                    .unwrap()
                    - constant_from!(1),
            ]
            .into_iter()
            .map(|expr| expr * enabled_cell.curr_expr(meta) * fixed_curr!(meta, step_sel))
            .collect::<Vec<_>>()
        });

        let sum_ops_expr_with_init = |init: Expression<F>,
                                      meta: &mut VirtualCells<'_, F>,
                                      get_expr: &dyn Fn(
            &mut VirtualCells<'_, F>,
            &Rc<Box<dyn EventTableOpcodeConfig<F>>>,
        ) -> Option<Expression<F>>| {
            op_bitmaps
                .iter()
                .filter_map(|(op, (lvl1, lvl2))| {
                    get_expr(meta, op_configs.get(op).unwrap()).map(|expr| {
                        expr * fixed_curr!(meta, step_sel)
                            * lvl1_bits[*lvl1].curr_expr(meta)
                            * lvl2_bits[*lvl2].curr_expr(meta)
                    })
                })
                .fold(init, |acc, x| acc + x)
        };

        let sum_ops_expr = |meta: &mut VirtualCells<'_, F>,
                            get_expr: &dyn Fn(
            &mut VirtualCells<'_, F>,
            &Rc<Box<dyn EventTableOpcodeConfig<F>>>,
        ) -> Option<Expression<F>>| {
            op_bitmaps
                .iter()
                .filter_map(|(op, (lvl1, lvl2))| {
                    get_expr(meta, op_configs.get(op).unwrap()).map(|expr| {
                        expr * fixed_curr!(meta, step_sel)
                            * lvl1_bits[*lvl1].curr_expr(meta)
                            * lvl2_bits[*lvl2].curr_expr(meta)
                    })
                })
                .reduce(|acc, x| acc + x)
                .unwrap()
        };

        meta.create_gate("c5a. rest_mops change", |meta| {
            vec![sum_ops_expr_with_init(
                rest_mops_cell.next_expr(meta) - rest_mops_cell.curr_expr(meta),
                meta,
                &|meta, config: &Rc<Box<dyn EventTableOpcodeConfig<F>>>| config.mops(meta),
            )]
            .into_iter()
            .map(|expr| expr * enabled_cell.curr_expr(meta) * fixed_curr!(meta, step_sel))
            .collect::<Vec<_>>()
        });

        meta.create_gate("c5b. rest_jops change", |meta| {
            vec![sum_ops_expr_with_init(
                rest_jops_cell.next_expr(meta) - rest_jops_cell.curr_expr(meta),
                meta,
                &|meta, config: &Rc<Box<dyn EventTableOpcodeConfig<F>>>| config.jops_expr(meta),
            )]
            .into_iter()
            .map(|expr| expr * enabled_cell.curr_expr(meta) * fixed_curr!(meta, step_sel))
            .collect::<Vec<_>>()
        });

        meta.create_gate("c5c. input_index change", |meta| {
            vec![sum_ops_expr_with_init(
                input_index_cell.curr_expr(meta) - input_index_cell.next_expr(meta),
                meta,
                &|meta, config: &Rc<Box<dyn EventTableOpcodeConfig<F>>>| {
                    config.input_index_increase(meta, &common_config)
                },
            )]
            .into_iter()
            .map(|expr| expr * enabled_cell.curr_expr(meta) * fixed_curr!(meta, step_sel))
            .collect::<Vec<_>>()
        });

        meta.create_gate("c5d. external_host_call_index change", |meta| {
            vec![sum_ops_expr_with_init(
                external_host_call_index_cell.curr_expr(meta)
                    - external_host_call_index_cell.next_expr(meta),
                meta,
                &|meta, config: &Rc<Box<dyn EventTableOpcodeConfig<F>>>| {
                    config.external_host_call_index_increase(meta, &common_config)
                },
            )]
            .into_iter()
            .map(|expr| expr * enabled_cell.curr_expr(meta) * fixed_curr!(meta, step_sel))
            .collect::<Vec<_>>()
        });

        meta.create_gate("c5e. sp change", |meta| {
            vec![sum_ops_expr_with_init(
                sp_cell.curr_expr(meta) - sp_cell.next_expr(meta),
                meta,
                &|meta, config: &Rc<Box<dyn EventTableOpcodeConfig<F>>>| config.sp_diff(meta),
            )]
            .into_iter()
            .map(|expr| {
                expr * enabled_cell.curr_expr(meta)
                    * enabled_cell.next_expr(meta)
                    * fixed_curr!(meta, step_sel)
            })
            .collect::<Vec<_>>()
        });

        meta.create_gate("c5f. mpages change", |meta| {
            vec![sum_ops_expr_with_init(
                mpages_cell.curr_expr(meta) - mpages_cell.next_expr(meta),
                meta,
                &|meta, config: &Rc<Box<dyn EventTableOpcodeConfig<F>>>| {
                    config.allocated_memory_pages_diff(meta)
                },
            )]
            .into_iter()
            .map(|expr| expr * enabled_cell.curr_expr(meta) * fixed_curr!(meta, step_sel))
            .collect::<Vec<_>>()
        });

        meta.create_gate("c6a. eid change", |meta| {
            vec![(eid_cell.next_expr(meta) - eid_cell.curr_expr(meta) - constant_from!(1))]
                .into_iter()
                .map(|expr| expr * enabled_cell.curr_expr(meta) * fixed_curr!(meta, step_sel))
                .collect::<Vec<_>>()
        });

        meta.create_gate("c6b. fid change", |meta| {
            vec![sum_ops_expr_with_init(
                fid_cell.curr_expr(meta) - fid_cell.next_expr(meta),
                meta,
                &|meta, config: &Rc<Box<dyn EventTableOpcodeConfig<F>>>| {
                    config
                        .next_fid(meta, &common_config)
                        .map(|x| x - fid_cell.curr_expr(meta))
                },
            )]
            .into_iter()
            .map(|expr| {
                expr * enabled_cell.curr_expr(meta)
                    * enabled_cell.next_expr(meta)
                    * fixed_curr!(meta, step_sel)
            })
            .collect::<Vec<_>>()
        });

        meta.create_gate("c6c. iid change", |meta| {
            vec![sum_ops_expr_with_init(
                iid_cell.next_expr(meta) - iid_cell.curr_expr(meta) - constant_from!(1),
                meta,
                &|meta, config: &Rc<Box<dyn EventTableOpcodeConfig<F>>>| {
                    config
                        .next_iid(meta, &common_config)
                        .map(|x| iid_cell.curr_expr(meta) + constant_from!(1) - x)
                },
            )]
            .into_iter()
            .map(|expr| expr * enabled_cell.curr_expr(meta) * fixed_curr!(meta, step_sel))
            .collect::<Vec<_>>()
        });

        meta.create_gate("c6d. frame_id change", |meta| {
            vec![sum_ops_expr_with_init(
                frame_id_cell.curr_expr(meta) - frame_id_cell.next_expr(meta),
                meta,
                &|meta, config: &Rc<Box<dyn EventTableOpcodeConfig<F>>>| {
                    config
                        .next_frame_id(meta, &common_config)
                        .map(|x| x - frame_id_cell.curr_expr(meta))
                },
            )]
            .into_iter()
            .map(|expr| expr * enabled_cell.curr_expr(meta) * fixed_curr!(meta, step_sel))
            .collect::<Vec<_>>()
        });

        meta.create_gate("c7. itable_lookup_encode", |meta| {
            let opcode = sum_ops_expr(
                meta,
                &|meta, config: &Rc<Box<dyn EventTableOpcodeConfig<F>>>| Some(config.opcode(meta)),
            );
            vec![
                (encode_instruction_table_entry(fid_cell.expr(meta), iid_cell.expr(meta), opcode)
                    - itable_lookup_cell.curr_expr(meta))
                    * fixed_curr!(meta, step_sel),
            ]
        });

        jtable.configure_in_table(meta, "c8a. itable_lookup in itable", |meta| {
            jtable_lookup_cell.curr_expr(meta) * fixed_curr!(meta, step_sel)
        });

        itable.configure_in_table(meta, "c8b. brtable_lookup in brtable", |meta| {
            itable_lookup_cell.curr_expr(meta) * fixed_curr!(meta, step_sel)
        });

        brtable.configure_in_table(meta, "c8c. jtable_lookup in jtable", |meta| {
            brtable_lookup_cell.curr_expr(meta) * fixed_curr!(meta, step_sel)
        });

        rtable.configure_in_pow_set(meta, "c8d. pow_table_lookup in pow_table", |meta| {
            pow_table_lookup_cell.curr_expr(meta) * fixed_curr!(meta, step_sel)
        });

        bit_table.configure_in_table(meta, "c8f: bit_table_lookup in bit_table", |meta| {
            bit_table_lookup_cell.curr_expr(meta) * fixed_curr!(meta, step_sel)
        });

        external_host_call_table.configure_in_table(
            meta,
            "c8e. external_foreign_call_lookup in foreign table",
            |meta| external_foreign_call_lookup_cell.curr_expr(meta) * fixed_curr!(meta, step_sel),
        );

        Self {
            step_sel,
            common_config,
            op_configs,
        }
    }
}

pub struct EventTableChip<F: FieldExt> {
    config: EventTableConfig<F>,
    max_available_rows: usize,
}

impl<F: FieldExt> EventTableChip<F> {
    pub(super) fn new(config: EventTableConfig<F>) -> Self {
        Self {
            config,
            max_available_rows: max_etable_rows() as usize / EVENT_TABLE_ENTRY_ROWS as usize
                * EVENT_TABLE_ENTRY_ROWS as usize,
        }
    }
}