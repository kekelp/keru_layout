use keru_layout::*;

fn stack(axis: Axis, spacing: f32, padding: f32) -> LayoutParams {
    LayoutParams {
        padding: Xy::new([padding, padding], [padding, padding]),
        children_layout: ChildrenLayout::Stack { arrange: Arrange::Start, axis, spacing },
        ..LayoutParams::DEFAULT
    }
}

fn text(size: Xy<Size>) -> LayoutParams {
    LayoutParams { size, text: TextContent::Wrapping, ..LayoutParams::DEFAULT }
}

fn main() {
    let mut tree: LayoutTree<&str> = LayoutTree::new();
    let root = tree.root();

    let hstack = tree.add_node(root, stack(Axis::X, 15.0, 15.0));
    tree.add_node(hstack, LayoutParams { size: Xy::new(Size::AspectRatio(0.5), Size::Fill), ..LayoutParams::DEFAULT });

    let vstack = tree.add_node(hstack, stack(Axis::Y, 0.0, 12.0));
    tree.add_node_with_context(vstack, text(Xy::new(Size::FitContent, Size::FitContent)), "Single line text");
    tree.add_node_with_context(vstack, text(Xy::new(Size::Fill, Size::FitContent)), "Paragraph with wrapping text. The height depends on the width. Paragraph with wrapping text. The height depends on the width. Paragraph with wrapping text. The height depends on the width. ");

    tree.solve_layout_with_measure(Xy::new(600.0, 450.0), measure_monospace_text);
    for warning in tree.warnings() {
        println!("Warning: {}", warning);
    }
    std::fs::write("example.svg", tree.to_svg()).unwrap();
    println!("Wrote example.svg");
}
