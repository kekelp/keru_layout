# `keru_layout`

`keru_layout` is a UI layout library that uses a new algorithm. It's used in the [Keru GUI library](https://github.com/kekelp/keru/), but it's available here as a self-contained crate.

## The Idea

Most layout algorithms will do one or more traversals of the tree of UI elements and calculate the size and position of each element as they go.

However, the kind of dependencies that layout libraries usually expose can go along the direction of the tree (`Frac`, `Fill`), but also in the opposite direction (`FitContent`), or "laterally" (`AspectRatio`, wrapping text).

In short, the idea of this library is to consider the resulting graph of dependencies and to solve it directly. The algorithm traverses the abstract dependency graph rather than the real UI tree.

## Advantages

- Performance: this library is very fast. [See this page for benchmarks](https://kekelp.github.io/arewelayoutyet/). Forked from [this repo](https://github.com/marc2332/arewelayoutyet) from the Torin author.

    `keru_layout` is the fastest or close to the fastest in all the categories that it supports. While other libraries have their strengths and their weaknesses, `keru_layout` is consistently fast in all cases. 
    
    It's likely that the other libraries are going faster when their top-down or bottom-up traversals of the UI tree happen to line up with the real structure of the dependency graph, and slower when the shapes don't match as well. By traversing the dependency graph directly, the new algorithm has more consistent performance regardless of the layout.

- Correctness: in some rare cases, such as layouts with many `AspectRatio` dependencies, a traditional tree-based algorithm would have to perform many traversals of the UI tree in order to finally compute the correct results. To avoid spending too much time on layout, many libraries will cut corners and silently produce an inconsistent layout.

    I wrote about one of these cases in this older [blog post](https://kekelp.github.io/keru/blog/layout/).

- Developer Experience: while it's usually not a problem to work around the correctness issues of traditional layout algorithms, it can still be confusing to run into them during development. Using a more consistent algorithm leads to fewer surprises and less confusion.

- Debuggability: almost all layout systems can express cyclic dependencies between elements, but in a traditional tree-based algorithm, this will result in a silently "broken" or inconsistent layout. With this algorithm, we can give the user a clear warning and an accurate list of the nodes that were stuck in cycles.

## Usage

`keru_layout` has a trait-based API similar to Taffy's low-level API.
There's also an alternative API similar to Taffy's high-level API where the library provides a ready-to-use `LayoutTree`, but it's only meant for quick experiments, examples and testing.

The trait-based API assumes that you already have a tree of UI elements, and that every UI element roughly corresponds to one layout element. Then, using the library looks like this:

1) Add a `LayoutParams` to each UI node. This is the struct that defines the node's layout role (size, position, stack, grid, etc).
2) Add a `NodeLayoutData` to each UI node. This is a small opaque struct that the solver uses to store per-node intermediate results and flags.

    >The layout solver could store all the `NodeLayoutData`s internally, but then it would have to maintain a mapping between your `NodeId`s and its own internal `NodeLayoutData` ids. If you help the solver out by storing the `NodeLayoutData`s in your own tree, it can go much faster.

    >If your UI nodes already have a lot of data in them, it may be good for performance to store the `LayoutParams` and `NodeLayoutData` separately: for example, in a `SecondaryMap` if the tree is a `SlotMap`, or in a parallel `Vec` if the tree is a `Slab`.

3) Add a single centralized `LayoutData` to your top level struct, or anywhere.

4) Implement the `TreeAdapter` trait for the whole UI tree:

    - Set the `NodeId` type to the Id that can be used to refer to nodes in the UI tree, such as a slotmap key or a slab index.
    - Fill in the `node_layout_params()`, `node_layout_data()`, `node_layout_data_mut()` and `layout_data()` to simply access the data corresponding to that `NodeId`.

    - Implement the `root()`, `parent()`, `first_child()`, `next_sibling()` methods so that the layout solver can read the preexisting tree links of your UI nodes.

    - Implement the `measure_text_width()` and `measure_text_height()` methods to measure the text contained in a node.
    If text measurement is not needed, for example if no node should fit to the size of text, it's fine to provide a dummy implementation.

    - Finally, implement the `write_output_rect()` callback, and make it write provided output rects where you need them to go, such as a `rect` field in the UI node.

    - Optionally, implement the `layout_warning()` callback to notify the user about any cycles or other issues in their layout.

Then, you can call the trait's `solve_layout()` method to calculate the layout and write all the output sizes.

### `LayoutTree` API

Create a `LayoutTree`, then insert nodes into the layout tree to match the structure of your UI tree, run `tree.solve_layout()`, then use `tree.layout()` on each node to extract the results and use them to update the corresponding UI node.

There's also a function that draws a SVG image to show the resulting layout.

```rust,no_run
use keru_layout::*;

let mut tree: LayoutTree = LayoutTree::new();
let row = tree.add_node(tree.root(), LayoutParams { size: Xy::new(Size::FitContent, Size::FitContent), ..LayoutParams::DEFAULT });
let item = tree.add_node(row, LayoutParams { size: Xy::new(Size::Pixels(100.0), Size::Pixels(20.0)), ..LayoutParams::DEFAULT });
tree.solve_layout(Xy::new(800.0, 600.0));
// Read the output rect for the "item" node
let rect = tree.layout(item);
// Draw the layout rectangles as SVG to inspect the layout
std::fs::write("example.svg", tree.to_svg()).unwrap();
```

## Incremental Relayout

The dependency graph approach is also quite useful for incremental relayout. This library supports incremental relayout for non-structural changes to cycle-free layouts. See the documentation of the `solve_layout_incremental()` method in the `TreeAdapter` trait.

Supporting structural changes wouldn't be impossible, but it would bring a lot of extra complexity. For example, a structural change can cause a cycle to form in a previously cycle-free tree.

Because the algorithm is quite fast, doing a full relayout instead of an incremental one is unlikely to be the reason why a program misses a frame. That means that incremental relayout only really helps with power usage and heat. So, it makes sense to focus on the kinds of relayouts that can happen at 60/120+ Hz, like elements being dragged or resized, and those are typically not structural.

If you don't want to use incremental relayout at all, you can turn off the `incremental` feature to take out all traces of it and improve the size and cache efficiency of the layout data. This can make the full solve about 1-5% faster.


## FAQ

- **Is this the same as the Cassowary constraint solving algorithm?**

    No, it's different. This algorithm doesn't model all dependencies as constraints, doesn't use the simplex method, or anything like that.


## Screenshots

![Layout example](https://raw.githubusercontent.com/kekelp/keru_layout/main/examples/example.svg "Svg output for the layout in `example.rs`")

*SVG output of the `example.rs` example*