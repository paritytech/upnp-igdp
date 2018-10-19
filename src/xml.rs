// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use roxmltree::Node;

pub(crate) struct Cursor<'a, 'd: 'a> {
    node: Option<Node<'a, 'd>>
}

impl<'a, 'd> Cursor<'a, 'd> {
    pub(crate) fn new(root: Node<'a, 'd>) -> Self {
        Cursor { node: Some(root) }
    }

    pub(crate) fn get(&self, name: &str) -> Self {
        Cursor {
            node: self.node.as_ref().and_then(|n| {
                n.children().find(|n| n.has_tag_name(name))
            })
        }
    }

    pub(crate) fn text(&self) -> Option<&str> {
        self.node.as_ref().and_then(|n| n.text())
    }
}


