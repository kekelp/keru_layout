use crate::*;
use crate::Axis::{X, Y};

/// What `solve_layout_incremental` did. The `Fallback` variants give the reason for a full solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncrementalRelayoutOutcome {
    /// The relayout was incremental.
    Success {
        /// Slot computations.
        computed: u32,
    },
    /// In `solve_layout_incremental_checked()`, the incremental relayout gave a different result than the full relayout. The tree keeps the result of the full relayout. This should only be possible if the tree had a structural change that wasn't registered with `register_node_change()`.
    CheckedIncrementalRelayoutMismatch,
    /// The incremental relayout fell back to a full relayout because the full layout was never solved before.
    FallbackNeverSolved,
    /// The incremental relayout fell back to a full relayout because the tree had a structural change. Structural changes are not supported by the incremental solver.
    FallbackStructuralChange,
    /// The incremental relayout fell back to a full relayout because the last full solve found a cycle. With cycles, the incremental solver can't guarantee identical results to the full solver in all cases.
    FallbackCycle,
    /// The incremental relayout fell back to a full relayout because the changes affected a large part of the tree, and the solver estimated it would be faster to fall back to a full relayout.
    FallbackTooMuchWork,
    /// The incremental relayout fell back to a full relayout because `LayoutTree::invalidate()` was called explicitly.
    FallbackInvalidated,
}

impl<I> LayoutData<I> {
    /// Records a change in the params of a node. A structural change makes the next relayout a full solve.
    pub(crate) fn record_node_change(&mut self, i: I, old: &LayoutParams, new: &LayoutParams) {
        if is_structural(old, new) {
            self.request_full_solve(IncrementalRelayoutOutcome::FallbackStructuralChange);
        } else {
            self.node_changes.push((i, *old));
        }
    }

    /// Makes the next relayout a full solve. An earlier reason stays.
    pub(crate) fn request_full_solve(&mut self, reason: IncrementalRelayoutOutcome) {
        self.pending_full_solve.get_or_insert(reason);
    }

    pub(crate) fn clear(&mut self) {
        self.node_changes.clear();
        self.content_changes.clear();
    }
}

/// True if the change can change the dependencies between the slots.
fn is_structural(old: &LayoutParams, new: &LayoutParams) -> bool {
    use std::mem::discriminant;
    let same_kind = |a: Option<Size>, b: Option<Size>| a.map(|a| discriminant(&a)) == b.map(|b| discriminant(&b));
    let same_layout = match (old.children_layout, new.children_layout) {
        (ChildrenLayout::Free, ChildrenLayout::Free) => true,
        (ChildrenLayout::Stack { axis: a, .. }, ChildrenLayout::Stack { axis: b, .. }) => a == b,
        // The `Count` grid cells are assigned in `prepare`.
        (ChildrenLayout::Grid { columns: MainAxisCellSize::Count(a), main_axis: main_a, backfill: backfill_a, .. }, ChildrenLayout::Grid { columns: MainAxisCellSize::Count(b), main_axis: main_b, backfill: backfill_b, .. }) => a == b && main_a == main_b && backfill_a == backfill_b,
        (ChildrenLayout::Grid { columns: MainAxisCellSize::Width(_), main_axis: main_a, backfill: backfill_a, .. }, ChildrenLayout::Grid { columns: MainAxisCellSize::Width(_), main_axis: main_b, backfill: backfill_b, .. }) => main_a == main_b && backfill_a == backfill_b,
        _ => false,
    };
    old.disabled != new.disabled
        || old.free_placement != new.free_placement
        || old.grid_span != new.grid_span
        || old.text != new.text
        || old.image.is_some() != new.image.is_some()
        || ! same_layout
        || [X, Y].into_iter().any(|axis| {
            ! same_kind(Some(old.size[axis]), Some(new.size[axis]))
                || ! same_kind(old.min_size[axis], new.min_size[axis])
                || ! same_kind(old.max_size[axis], new.max_size[axis])
        })
}

pub(crate) trait Incremental: Solve {
    fn measure_sizes_incremental(&mut self, window_size: Xy<f32>) -> IncrementalRelayoutOutcome {
        let root = self.root();
        let data = self.layout_data();
        // Take out the changes and content_changes
        let mut changes = std::mem::take(&mut data.node_changes);
        let mut content_changes = std::mem::take(&mut data.content_changes);
        let last_window_size = std::mem::replace(&mut data.window_size, window_size);
        let max_computed = data.n_nodes_solved / 2;
        data.cursor = 0;

        let outcome;
        if changes.len() + content_changes.len() > max_computed as usize {
        // Too many changes, so give up on incremental relayout.
            outcome = IncrementalRelayoutOutcome::FallbackTooMuchWork;
        } else {
            // The `Frac` and `Fill` sizes of the root read the window size.
            for axis in [X, Y] {
                if last_window_size[axis] != window_size[axis] {
                    self.queue_declared(root, axis, |size| size.is_flex());
                }
            }
            for (i, old) in &changes {
                self.queue_node_change(*i, old);
            }
            for &i in &content_changes {
                self.queue_content_change(i);
            }
            outcome = self.run_queue(max_computed);
        }

        // Put the vecs with the allocations back in place
        changes.clear();
        content_changes.clear();
        let data = self.layout_data();
        data.node_changes = changes;
        data.content_changes = content_changes;
        for bucket in &mut data.buckets {
            bucket.clear();
        }
        outcome
    }

    /// Records a change in the params of a node.
    fn incr_register_node_change(&mut self, i: Self::NodeId, old: &LayoutParams) {
        let new = *self.node_layout_params(i);
        self.layout_data().record_node_change(i, old, &new);
    }

    /// Records that the text or the image of a node changed.
    fn incr_register_content_change(&mut self, i: Self::NodeId) {
        self.layout_data().content_changes.push(i);
    }

    /// Does an incremental relayout, or a full solve if the registered changes need one. If `verify` is true, it also does a full solve and compares the sizes.
    fn solve_incremental(&mut self, window_size: Xy<f32>, verify: bool) -> IncrementalRelayoutOutcome {
        let data = self.layout_data();
        let outcome = if let Some(reason) = data.pending_full_solve {
            reason
        } else if data.has_cycle {
            IncrementalRelayoutOutcome::FallbackCycle
        } else {
            self.measure_sizes_incremental(window_size)
        };
        match outcome {
            IncrementalRelayoutOutcome::Success { .. } => match verify {
                true => {
                    if ! self.run_full_solve_and_compare(window_size) {
                        return IncrementalRelayoutOutcome::CheckedIncrementalRelayoutMismatch;
                    }
                }
                false => self.place_nodes(),
            },
            _ => self.solve_layout(window_size),
        }
        outcome
    }

    fn run_full_solve_and_compare(&mut self, window_size: Xy<f32>) -> bool {
        let mut incremental = Vec::new();
        let mut next = Some(self.root());
        while let Some(i) = next {
            incremental.push((i, self.node_layout_data(i).values.map(f32::to_bits)));
            next = self.tree_traversal_next(i);
        }
        self.solve_layout(window_size);
        for (i, values) in incremental {
            if self.node_layout_data(i).values.map(f32::to_bits) != values {
                return false;
            }
        }
        true
    }

    /// Queues the slots that a change in the params of a node can affect.
    fn queue_node_change(&mut self, i: Self::NodeId, old: &LayoutParams) {
        let new_params = *self.node_layout_params(i);
        let new_params = &new_params;
        // If a node went from disabled to enabled or viceversa, that's a structural change, and it won't reach this function at all.
        // Seeing it here just means that the node was disabled both before and after and can be ignored.
        if new_params.disabled {
            return;
        }

        for axis in [X, Y] {
            for kind in DECLARED_SIZES {
                let read = |params: &LayoutParams| match kind {
                    Kind::Min => params.min_size[axis],
                    Kind::Max => params.max_size[axis],
                    _ => Some(params.size[axis]),
                };
                if read(old) != read(new_params) {
                    self.queue(Slot { node: i, axis, kind });
                }
            }
            // A `FitContent` stack reads the `Frac` numbers of its children from their params, without a slot.
            if old.size[axis] != new_params.size[axis] && matches!(new_params.size[axis], Size::Frac(_)) && ! new_params.free_placement && let Some(parent) = self.parent(i) {
                self.queue_declared(parent, axis, |size| size == Size::FitContent);
            }
        }

        for axis in [X, Y] {
            if old.padding[axis] != new_params.padding[axis] {
                self.queue_inner_size_readers(i, axis);
            }
        }

        if old.fill_weight != new_params.fill_weight && ! new_params.free_placement && let Some(parent) = self.parent(i) {
            if let Some(axis) = self.stack_axis(parent) {
                self.queue_declared(i, axis, |size| size.is_fill());
                self.queue_even_share(parent);
            }
        }

        // Only the spacing or the cell size can change. `arrange` changes only the placement.
        match (old.children_layout, new_params.children_layout) {
            (ChildrenLayout::Stack { spacing: a, .. }, ChildrenLayout::Stack { axis, spacing: b, .. }) if a != b => {
                self.queue_inner_size_readers(i, axis);
            }
            (old_layout @ ChildrenLayout::Grid { .. }, new_layout @ ChildrenLayout::Grid { .. }) if old_layout != new_layout => {
                self.queue_inner_size_readers(i, X);
                self.queue_inner_size_readers(i, Y);
            }
            _ => {}
        }

        if old.image != new_params.image {
            self.queue_content_change(i);
        }
    }

    fn queue_content_change(&mut self, i: Self::NodeId) {
        if self.node_layout_params(i).disabled {
            return;
        }
        for axis in [X, Y] {
            self.queue_declared(i, axis, |size| size == Size::FitContent);
        }
    }

    fn queue_inner_size_readers(&mut self, i: Self::NodeId, axis: Axis) {
        let grid_main = self.width_grid_main_axis(i);
        // In a grid, the spacing and the cell size on one axis can change the cells on the other axis.
        let axes: &[Axis] = if grid_main.is_some() { &[X, Y] } else { &[axis] };
        for &axis in axes {
            self.queue_declared(i, axis, |size| size == Size::FitContent);
            for_each_layout_child!(self, i, child, {
                let in_stack = self.is_in_stack(child, axis);
                self.queue_declared(child, axis, |size| matches!(size, Size::Frac(_)) || (size.is_fill() && ! in_stack));
            });
        }
        if let Some(main) = grid_main {
            self.queue(Slot { node: i, axis: main, kind: Kind::Final });
        }
        // Wrapped text reads the inner width for the height.
        if axes.contains(&X) && self.text_wraps(i, Y) {
            self.queue_declared(i, Y, |size| size == Size::FitContent);
        }
        self.queue_even_share(i);
    }

    fn queue_even_share(&mut self, i: Self::NodeId) {
        if let Some(axis) = self.stack_axis(i) {
            self.queue(Slot { node: i, axis, kind: Kind::EvenShare });
        }
    }

    fn queue(&mut self, slot: Slot<Self::NodeId>) {
        let Slot { node, axis, kind } = slot;
        let data = self.node_layout_data_mut(node);
        if data.flags.check_state(axis, kind) != SOLVED {
            return;
        }
        data.flags.set_state(axis, kind, UNSOLVED);
        let level = data.levels[slot_index(axis, kind)] as usize;
        let data = self.layout_data();
        debug_assert!(level >= data.cursor, "Slot {slot:?} queued at level {level}, below the current level {}.", data.cursor);
        while data.buckets.len() <= level {
            data.buckets.push(Vec::new());
        }
        data.buckets[level].push(slot);
    }

    /// Computes the queued slots in order of level, and queues the dependents of each slot that changes. The levels are exact, so each slot is computed once. It stops after `max_computed` slot computations.
    fn run_queue(&mut self, max_computed: u32) -> IncrementalRelayoutOutcome {
        let mut computed = 0;
        loop {
            let data = self.layout_data();
            if data.cursor >= data.buckets.len() {
                break;
            }
            let cursor = data.cursor;
            let Some(slot) = data.buckets[cursor].pop() else {
                data.cursor += 1;
                continue;
            };
            computed += 1;
            if computed > max_computed {
                return IncrementalRelayoutOutcome::FallbackTooMuchWork;
            }
            let k = slot_index(slot.axis, slot.kind);
            let old = self.node_layout_data(slot.node);
            let (old_value, old_grid_lines) = (old.values[k], old.grid_lines);
            // The level of the slot stays the same.
            self.finish_slot(slot);
            let new = self.node_layout_data(slot.node);
            // The cells of a width grid can change while its size stays the same.
            if new.values[k].to_bits() != old_value.to_bits() || new.grid_lines != old_grid_lines {
                self.queue_dependents(slot);
            }
        }
        IncrementalRelayoutOutcome::Success { computed }
    }

    /// Queues every slot that reads `slot`. It is the reverse of `solve_all_dependencies`, and it has to stay in sync with it.
    fn queue_dependents(&mut self, slot: Slot<Self::NodeId>) {
        let Slot { node: i, axis, kind } = slot;
        match kind {
            Kind::EvenShare => {
                for_each_layout_child!(self, i, child, {
                    if self.is_in_stack(child, axis) {
                        self.queue_declared(child, axis, |size| size.is_fill());
                    }
                });
            }
            Kind::Final => {
                let other = axis.other();
                self.queue_declared(i, other, |size| matches!(size, Size::AspectRatio(_)));
                let grid_main = self.width_grid_main_axis(i) == Some(axis);
                if grid_main || (axis == X && self.text_wraps(i, Y)) {
                    self.queue_declared(i, other, |size| size == Size::FitContent);
                }
                if self.stack_axis(i) == Some(axis) {
                    self.queue(Slot { node: i, axis, kind: Kind::EvenShare });
                }
                for_each_layout_child!(self, i, child, {
                    for child_axis in [X, Y] {
                        // A placed child of a width grid also reads the main axis for its other axis.
                        if child_axis == axis || (grid_main && ! self.node_layout_params(child).free_placement) {
                            let in_stack = self.is_in_stack(child, child_axis);
                            self.queue_declared(child, child_axis, |size| matches!(size, Size::Frac(_)) || (size.is_fill() && ! in_stack));
                        }
                    }
                });
                if let Some(parent) = self.stack_parent(i, axis) && ! self.any_size_is_fill(i, axis) {
                    self.queue(Slot { node: parent, axis, kind: Kind::EvenShare });
                }
            }
            declared => {
                self.queue(Slot { node: i, axis, kind: Kind::Final });
                if ! self.node_layout_params(i).free_placement && let Some(parent) = self.parent(i) {
                    let size = self.read_size(i, axis, declared).unwrap();
                    if self.stack_axis(parent) == Some(axis) && self.any_size_is_fill(i, axis) && ! size.is_fill() {
                        self.queue(Slot { node: parent, axis, kind: Kind::EvenShare });
                    }
                    if ! size.is_flex() {
                        self.queue_declared(parent, axis, |size| size == Size::FitContent);
                    }
                }
            }
        }
    }

    /// Queues the declared slots of a node on `axis` whose size matches `filter`.
    #[inline(always)]
    fn queue_declared(&mut self, i: Self::NodeId, axis: Axis, filter: impl Fn(Size) -> bool) {
        for kind in DECLARED_SIZES {
            if let Some(size) = self.read_size(i, axis, kind) && filter(size) {
                self.queue(Slot { node: i, axis, kind });
            }
        }
    }
}

impl<T: TreeAdapter + ?Sized> Incremental for T {}
