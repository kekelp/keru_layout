#![doc = include_str!("../README.md")]

#![warn(missing_docs)]

#[macro_use]
mod solve;
mod place;
#[cfg(feature = "incremental")]
mod incremental;
#[cfg(feature = "incremental")]
mod layout_tree;


use std::fmt::Debug;
use std::ops::{Index, IndexMut};

pub use solve::NodeLayoutData;
pub(crate) use solve::*;
#[cfg(feature = "incremental")]
pub(crate) use incremental::*;
pub(crate) use place::Place;
#[cfg(feature = "incremental")]
pub use incremental::IncrementalRelayoutOutcome;
#[cfg(feature = "incremental")]
pub use layout_tree::{LayoutTree, NodeId, measure_monospace_text};

/// The adapter trait that allows the layout algorithm to read your UI tree and compute the layout for it.
/// 
/// See the top-level crate documentation, or [`LayoutTree`] for an example of a tree that implements the trait.
pub trait TreeAdapter {
    /// An id type that can be used to refer to nodes in your UI tree.
    type NodeId: Copy + Eq + Debug;
    /// The `NodeId` that refers to the root node, which should correspond to the whole window.
    fn root(&self) -> Self::NodeId;
    /// Get the `NodeId` for a node's parent.
    fn parent(&self, i: Self::NodeId) -> Self::NodeId;
    /// Get the `NodeId` for a node's first child, or `None` if it has no children.
    fn first_child(&self, i: Self::NodeId) -> Option<Self::NodeId>;
    /// Get the `NodeId` for a node's next sibling, or `None` if it's the last child of its parent.
    fn next_sibling(&self, i: Self::NodeId) -> Option<Self::NodeId>;
    /// Read a the [`LayoutParams`] of the node corresponding to `i`.
    fn node_layout_params(&self, i: Self::NodeId) -> &LayoutParams;
    /// Access the [`NodeLayoutData`] of the node corresponding to `i`.
    fn node_layout_data(&self, i: Self::NodeId) -> &NodeLayoutData;
    /// Get a mutable reference to the [`NodeLayoutData`] of the node corresponding to `i`.
    fn node_layout_data_mut(&mut self, i: Self::NodeId) -> &mut NodeLayoutData;
    /// The layout data for the whole tree.
    fn layout_data(&mut self) -> &mut LayoutData<Self::NodeId>;
    /// The width of the text in a node, without any wrapping, in logical pixels.
    fn measure_text_width(&mut self, i: Self::NodeId) -> f32;
    
    /// The height of the text in a node, with its width optionally fixed to `wrap_width`, in logical pixels.
    fn measure_text_height(&mut self, i: Self::NodeId, wrap_width: Option<f32>) -> f32;
    /// Called when the layout solver computes the end result for a node's rectangle.
    /// The output is provided in both absolute coordinates and relative to the parent, in logical pixels.
    fn write_output_rect(&mut self, i: Self::NodeId, rect: XyRect, local_rect: XyRect);

    /// The size of the window, in logical pixels.
    fn window_size(&self) -> Xy<f32>;
    
    /// Called when the layout solver finds a problem in a node's layout.
    /// 
    /// Depending on the use case, this can be reported to the user.
    fn layout_warning(&mut self, _warning: LayoutWarning<Self::NodeId>) {}

    /// Solves the layout on all nodes.
    fn solve_layout(&mut self) {
        #[cfg(feature = "incremental")]
        self.layout_data().clear();
        self.measure_sizes();
        self.place_nodes();
    }

    /// Tries to solve the layout with a faster incremental algorithm based on the tree changes registered with `register_node_change()` and `register_content_change()`.
    /// 
    /// The results should always be the same as a full relayout, provided that all the changes were registered. To catch missing register calls, you can use `solve_layout_incremental_checked()` in debug builds.
    /// 
    /// The incremental solver will fall back to a full relayout internally if the tree has layout cycles, if the registered changes changed the structure of the tree rather than just the parameters, or if it heuristically estimated that so many nodes changed that the incremental relayout might be slower than a full one.
    /// 
    /// It's also totally fine to ignore this method, don't bother registering changes at all, and run the full `solve_layout()` every frame, as it is quite fast on its own.
    /// 
    /// Incremental relayout is sort of overrated. Optimizing things like a popup showing up with incremental relayout is usually not worth the complexity, which is why `solve_layout_incremental()` doesn't support structural changes at all. The only case where it might be worth it is to optimize dragged elements or layout-based animations, which actually force the layout to run at 60 or 120+ Hz, and the power or heat savings might be meaningful. Even in that case, it's likely that they will be overshadowed by other per-frame costs like rendering.
    /// 
    /// Currently only the size-measuring part of the layout is incremental, while the placement isn't.
    #[cfg(feature = "incremental")]
    fn solve_layout_incremental(&mut self) -> IncrementalRelayoutOutcome {
        self.solve_incremental(false)
    }

    /// Does `solve_layout_incremental`, then a full solve, and returns `IncrementalRelayoutOutcome::CheckedIncrementalRelayoutMismatch` if the result is different.
    #[cfg(feature = "incremental")]
    fn solve_layout_incremental_checked(&mut self) -> IncrementalRelayoutOutcome {
        self.solve_incremental(true)
    }

    /// Records that the params of a node changed for incremental relayouts. Call it after the tree has the new params. A structural change, such as a different size kind, makes the next relayout a full solve.
    #[cfg(feature = "incremental")]
    fn register_node_change(&mut self, i: Self::NodeId, old: &LayoutParams) {
        self.incr_register_node_change(i, old);
    }

    /// Records that the text or other intrinsic content of a node changed for incremental relayouts.
    #[cfg(feature = "incremental")]
    fn register_content_change(&mut self, i: Self::NodeId) {
        self.incr_register_content_change(i);
    }

    /// Places a free-placement node into a new position. Can be used to move scroll bars without requiring a full relayout. `parent_rect` is in logical pixels.
    fn place_free_placed_subtree(&mut self, node: Self::NodeId, parent_rect: XyRect) {
        self.place_free_node(node, parent_rect);
    }
}

/// The X or Y axes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Axis {
    /// The X axis.
    X,
    /// The Y axis.
    Y,
}

impl Axis {
    /// The axis orthogonal to `self`: Y if `self` is X and X if `self` is Y.
    pub fn other(&self) -> Self {
        match self {
            Axis::X => Axis::Y,
            Axis::Y => Axis::X,
        }
    }
}

pub(crate) use Axis::{X, Y};

/// A generic container for two-dimensional data.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Xy<T> {
    /// The X component.
    pub x: T,
    /// The Y component.
    pub y: T,
}

impl<T> Xy<T> {
    /// Create a new `Xy<T>`.
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl<T: Copy> Xy<T> {
    /// Create a new `Xy<T>` with the same value on both axes.
    pub const fn new_symm(v: T) -> Self {
        Self { x: v, y: v }
    }
}

impl<T> Index<Axis> for Xy<T> {
    type Output = T;
    #[inline(always)]
    fn index(&self, axis: Axis) -> &T {
        match axis {
            Axis::X => &self.x,
            Axis::Y => &self.y,
        }
    }
}

impl<T> IndexMut<Axis> for Xy<T> {
    #[inline(always)]
    fn index_mut(&mut self, axis: Axis) -> &mut T {
        match axis {
            Axis::X => &mut self.x,
            Axis::Y => &mut self.y,
        }
    }
}

/// A 2D rectangle, expressed as a start and end coordinates on each axis.
/// 
/// Alias of `Xy<[f32; 2]>`.
pub type XyRect = Xy<[f32; 2]>;

#[derive(Debug, Clone, Copy, PartialEq)]
/// The size of a layout element.
pub enum Size {
    /// A fixed size in logical pixels.
    Pixels(f32),
    /// A fraction of the parent element.
    /// 
    /// Collapses to zero if the parent has a size of zero, or if it's `FitContent` and has no other children that can give it a size. 
    Frac(f32),
    /// Fill all the available space in the parent element.
    /// 
    /// Multiple `Fill` children in a stack share the space evenly, or according to their `fill_weight` if it's different from the default of `1.0`.
    /// 
    /// Collapses to zero if the parent has a size of zero, or if it's `FitContent` and has no other children that can give it a size. 
    Fill,
    /// As big as the element's children or the intrinsic content such as text, plus the `padding`.
    FitContent,
    /// A size that depends on the size of this same element in the other axis, according to an aspect ratio.
    AspectRatio(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// The "origin" of a layout element inside its own rectangle.
/// 
/// For example, an element with `Size::Pixels(10.0)` and `Pos::Pixels(0.0)` on an axis will be placed at `[0.0, 10.0]`, `[-5.0, 5.0]` or `[-10.0, 0.0]` if its `Anchor` is `Start`, `Center` or `End` respectively.
/// 
/// Has no effect in a stack's main axis, or for `Pos` values other than `Pixels` and `Frac`.
pub enum Anchor {
    /// The left or top edge of the element's rectangle.
    Start,
    /// The center of the element's rectangle.
    Center,
    /// The right or bottom edge of the element's rectangle.
    End,
    /// Any fraction, with `0.0` corresponding to the left or top edge and `1.0` corresponding to the right or bottom edge.
    Frac(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// The position of a layout element in the space of its parent.
/// 
/// If the node is a child of a stack, its `Pos` value on the stack's main axis is ignored, as the stack positions the child according to its own logic.
/// 
/// In a grid, the `Pos` value controls how the node is placed within the space of a grid cell.
pub enum Pos {
    /// The left or top side of the parent.
    Start,
    /// The center of the parent.
    Center,
    /// The right or bottom side of the parent.
    End,
    /// A fixed number of pixels starting from the left or top side of the parent.
    Pixels(f32),
    /// A fraction of the parent's size starting from its left or top side.
    Frac(f32),
}

/// A warning about a potential problem in the user's layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LayoutWarning<Id> {
    /// A node is stuck in a dependency cycle.
    Cycle {
        /// The id of the node stuck in the cycle.
        node: Id,
        /// Which of the node's dimensions is stuck in the cycle.
        axis: Axis
    },
    /// The node's size is `AspectRatio` on both axes.
    /// 
    /// This also shows up as a `Cycle`.
    AspectRatioOnBothAxes {
        /// The id of the problematic node.
        node: Id,
    },
    /// A `FitContent` node only has `Fill` or `Frac` children, so its size collapsed to zero.
    FitContentCollapsed {
        /// The id of the problematic node.
        node: Id,
        /// Which of the node's dimensions the problem was found in.
        axis: Axis
    },
    /// The node's min size is bigger than its max size.
    MinAboveMax {
        /// The id of the problematic node.
        node: Id,
        /// Which of the node's dimensions the problem was found in.
        axis: Axis
    },
    /// A `FitContent` node couldn't determine consistent sizes for its `Frac` children, because they add up to more than 100%.
    ImpossibleFracChildren {
        /// The id of the problematic node.
        node: Id,
        /// Which of the node's dimensions the problem was found in.
        axis: Axis
    },
    /// The node's size is infinite or NaN.
    NotFinite {
        /// The id of the problematic node.
        node: Id,
        /// Which of the node's dimensions the problem was found in.
        axis: Axis
    },
}

impl<Id: Debug> std::fmt::Display for LayoutWarning<Id> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutWarning::Cycle { node, axis } => write!(f,
                "The {axis:?} size of node '{node:?}' is stuck in a dependency cycle."
            ),
            LayoutWarning::AspectRatioOnBothAxes { node } => write!(f,
                "Node '{node:?}' is `AspectRatio` on both axes. This will also show up as a cycle."
            ),
            LayoutWarning::FitContentCollapsed { node, axis } => write!(f,
                "The {axis:?} size of node '{node:?}' is `FitContent`, but it only has `Fill` or `Frac` children and no intrinsic content, so its size collapsed to zero."
            ),
            LayoutWarning::MinAboveMax { node, axis } => write!(f,
                "On the {axis:?} axis, the '{node:?}' node's min size is bigger than its max size. "
            ),
            LayoutWarning::ImpossibleFracChildren { node, axis } => write!(f,
                "On the {axis:?} axis, the '{node:?}' `FitContent` node couldn't determine consistent sizes for its `Frac` children, because they add up to more than 100%."
            ),
            LayoutWarning::NotFinite { node, axis } => write!(f,
                "The {axis:?} size of node '{node:?}' is infinite or NaN."
            ),
        }
    }
}

/// What to do if a freely placed node would extend past the edges of the window. It has no effect on the children of a `Stack` or a `Grid`, unless they are `free_placement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WindowOverflow {
    /// Leave the node where it is, even if it extends past the window edge.
    #[default]
    Ignore,
    /// Push the node back inside the window.
    Clamp,
    /// Mirror the node to the other side of its anchor point, as a right-click context menu does. If the mirrored node also doesn't fit, push it back inside the window.
    Flip,
}

/// The way that a stack's children are arranged along its main axis.
#[derive(Debug, Clone, Copy, Hash, PartialEq)]
pub enum Arrange {
    /// The children are arranged close together at the start of the stack.
    Start,
    /// The children are arranged close together at the end of the stack.
    End,
    /// The children are arranged close together in the center of the stack.
    Center,
    /// The free space is divided equally between the children, with no space at the edges.
    SpaceBetween,
    /// Each child gets an equal amount of free space on both sides, so the space at the edges is half the space between the children.
    SpaceAround,
    /// The free space is divided equally between the children and the edges.
    SpaceEvenly,
}

/// What determines the size and number of cells on the main axis of a grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MainAxisCellSize {
    /// Fix the number of cells and derive the cell size from the grid size.  
    Count(u32),
    /// Fix the size of the cells and derive the cell count from the grid size.
    /// 
    /// If the grid is `FitContent` on the main axis, the grid will only have one cell per row/column.  
    Width(f32),
}

/// Whether the node is a stack or a grid that arranges its children according to a specific rule.
/// 
/// Individual children can override this by setting `free_placement = true`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChildrenLayout {
    /// Every child can position itself freely according to its `Pos`.
    Free,
    /// Children are arranged on a stack.
    /// 
    /// This means that the children's `Pos` value on the stack's axis is ignored, but it can still be used to control alignment on the cross axis. 
    Stack {
        /// The way that the stack's children are arranged along its main axis.
        arrange: Arrange,
        /// The "main" axis along which the children are stacked.
        axis: Axis,
        /// The spacing between the stacked children.
        spacing: f32,
    },
    /// Children are arranged in a grid.
    ///
    /// The `Pos` value of the children on both axes can still be used to place them within the space of the grid cell.
    Grid {
        /// What determines the size and number of cells on the main axis of the grid.
        columns: MainAxisCellSize,
        /// The spacing between grid elements along the `X` axis.
        spacing_x: f32,
        /// The spacing between grid elements along the `Y` axis.
        spacing_y: f32,
        /// The axis along which the children are arranged.
        main_axis: Axis,
        /// Whether later children can fill a gap left by previous children.
        /// 
        /// Gaps can only be left when elements have span greater than the default of one.
        backfill: bool,
    },
}

/// The text content of a node, if any. The solver will measure it with `TreeAdapter::measure_text_width()` and `TreeAdapter::measure_text_height()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextContent {
    /// The node has no text.
    None,
    /// Text that wraps. Its height depends on the node's width.
    Wrapping,
    /// Text that doesn't wrap. Its width and height don't depend on the node's size.
    Fixed,
}

/// The parameters that define the layout behavior of a node, such as its size, position, whether it's a stack or a grid, etc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutParams {
    /// The position of a layout node in the space of its parent.
    pub position: Xy<Pos>,
    /// The size of a layout element on its two axes.
    pub size: Xy<Size>,
    /// The minimum size of a layout element on its two axes.
    pub min_size: Xy<Option<Size>>,
    /// The maximum size of a layout element on its two axes.
    pub max_size: Xy<Option<Size>>,
    /// The space that this node keeps for itself as padding before giving space to its children.
    pub padding: Xy<[f32; 2]>,
    /// Whether the node is a stack of a grid.
    pub children_layout: ChildrenLayout,
    /// Whether this children should ignore the parent's `ChildrenLayout` and place itself freely even if the parent is a stack or a grid.
    pub free_placement: bool,
    /// Whether this node should occupy more than one cell if this node is a child of a grid.
    pub grid_span: Xy<u16>,
    /// The text content of this node, if any.
    pub text: TextContent,
    /// The image or fixed-size content of a node, if any. 
    pub image: Option<Xy<f32>>,
    /// The "origin" of a layout element inside its own rectangle.
    pub anchor: Xy<Anchor>,
    /// If true for one axis, children elements are placed starting from the right or bottom edge. If the node is a stack or a grid, the order in which children are arranged is reversed. 
    pub children_origin_flipped: Xy<bool>,
    /// The node's behavior if it extends past the edges of the window.
    pub window_overflow: Xy<WindowOverflow>,
    /// In a stack, the `Fill` children share the free space in proportion to their weight. It has no effect on the other axis.
    pub fill_weight: f32,
    /// True if a node should be ignored by the layout solver. When a node is disabled, all its descendants must be disabled too.
    pub disabled: bool,

}

impl LayoutParams {
    /// Default values for the `LayoutParams`.
    pub const DEFAULT: LayoutParams = LayoutParams {
        size: Xy { x: Size::FitContent, y: Size::FitContent },
        min_size: Xy { x: None, y: None },
        max_size: Xy { x: None, y: None },
        fill_weight: 1.0,
        padding: Xy { x: [5.0, 5.0], y: [5.0, 5.0] },
        children_layout: ChildrenLayout::Free,
        free_placement: false,
        grid_span: Xy { x: 1, y: 1 },
        text: TextContent::None,
        image: None,
        disabled: false,
        position: Xy { x: Pos::Center, y: Pos::Center },
        anchor: Xy { x: Anchor::Center, y: Anchor::Center },
        children_origin_flipped: Xy { x: false, y: false },
        window_overflow: Xy { x: WindowOverflow::Ignore, y: WindowOverflow::Ignore },
    };
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Layout data for the whole tree. Without the `incremental` feature, it only holds scratch memory for the solver.
#[derive(Debug, Clone)]
pub struct LayoutData<I> {
    pub(crate) fills_scratch: Vec<FillChild>,
    pub(crate) occupied_scratch: Vec<bool>,

    #[cfg(feature = "incremental")]
    pub(crate) node_changes: Vec<(I, LayoutParams)>,
    #[cfg(feature = "incremental")]
    pub(crate) content_changes: Vec<I>,
    #[cfg(feature = "incremental")]
    pub(crate) pending_full_solve: Option<IncrementalRelayoutOutcome>,
    #[cfg(feature = "incremental")]
    pub(crate) last_window_size: Xy<f32>,
    #[cfg(feature = "incremental")]
    pub(crate) has_cycle: bool,
    #[cfg(feature = "incremental")]
    pub(crate) n_nodes_solved: u32,
    /// During a full solve, 1 + the highest level of the dependencies of the slot that is being solved.
    #[cfg(feature = "incremental")]
    pub(crate) current_level: u16,
    #[cfg(feature = "incremental")]
    pub(crate) buckets: Vec<Vec<Slot<I>>>,
    #[cfg(feature = "incremental")]
    pub(crate) cursor: usize,

    #[cfg(not(feature = "incremental"))]
    _marker: std::marker::PhantomData<I>,
}

impl<I> LayoutData<I> {
    /// Creates a new [`LayoutData`].
    pub const fn new() -> Self {
        LayoutData {
            #[cfg(feature = "incremental")]
            node_changes: Vec::new(),
            #[cfg(feature = "incremental")]
            content_changes: Vec::new(),
            #[cfg(feature = "incremental")]
            pending_full_solve: Some(IncrementalRelayoutOutcome::FallbackNeverSolved),
            #[cfg(feature = "incremental")]
            last_window_size: Xy::new(0.0, 0.0),
            #[cfg(feature = "incremental")]
            has_cycle: false,
            #[cfg(feature = "incremental")]
            n_nodes_solved: 0,
            #[cfg(feature = "incremental")]
            current_level: 0,
            #[cfg(feature = "incremental")]
            buckets: Vec::new(),
            #[cfg(feature = "incremental")]
            cursor: 0,
            fills_scratch: Vec::new(),
            occupied_scratch: Vec::new(),
            #[cfg(not(feature = "incremental"))]
            _marker: std::marker::PhantomData,
        }
    }
}

impl<I> Default for LayoutData<I> {
    fn default() -> Self {
        Self::new()
    }
}
