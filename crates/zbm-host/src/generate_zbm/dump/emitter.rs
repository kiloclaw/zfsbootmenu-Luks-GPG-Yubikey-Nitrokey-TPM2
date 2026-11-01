//! The block-style emitter: turns the events of a document into YAML text,
//! with the indentation, anchors and empty collections YAML::PP writes.

use super::style::{Style, best_style, double_quoted, literal, single_quoted};

/// One level of the emitter's state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Document,
    Mapping,
    MappingValue,
    Sequence,
    ComplexKey,
    ComplexValue,
}

#[derive(Clone, Debug)]
struct Frame {
    kind: Kind,
    index: usize,
    indent: String,
    newline: bool,
    column: bool,
    zero_indent: bool,
}

/// The block-style part of YAML::PP's emitter, with its indentation of two
/// spaces.
#[derive(Default)]
pub(super) struct Emitter {
    pub(super) output: String,
    column: usize,
    stack: Vec<Frame>,
    open_ended: bool,
}

const INDENT: &str = "  ";

impl Emitter {
    fn write(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let last_line = text.rsplit('\n').next().unwrap_or_default();
        if last_line.is_empty() {
            self.column = 0;
        } else if text.contains('\n') {
            self.column = last_line.chars().count();
        } else {
            self.column += last_line.chars().count();
        }
        self.output.push_str(text);
    }

    fn last(&mut self) -> &mut Frame {
        self.stack
            .last_mut()
            .expect("events arrive inside a document")
    }

    pub(super) fn document_start(&mut self) {
        let column = self.column != 0;
        self.stack = vec![Frame {
            kind: Kind::Document,
            index: 0,
            indent: String::new(),
            newline: false,
            column,
            zero_indent: false,
        }];
    }

    pub(super) fn document_end(&mut self) {
        self.stack.clear();
        if self.open_ended {
            self.write("...\n");
            self.open_ended = false;
        } else {
            self.open_ended = true;
        }
    }

    /// The common start of a block mapping or sequence: the `-`, `?` or `:` that
    /// introduces it, and its anchor. Returns the new level's indentation, whether
    /// its first entry starts on a new line, and whether a sequence is not
    /// indented.
    fn collection_start(&mut self, anchor: Option<usize>, sequence: bool) -> (String, bool, bool) {
        let mut yaml = String::new();
        let last = self
            .stack
            .last_mut()
            .expect("events arrive inside a document");
        let indent = last.indent.clone();
        let mut new_indent = indent.clone();
        let mut newline = false;
        let mut zero_indent = false;
        if last.kind == Kind::Document {
            newline = last.newline;
        } else {
            if last.newline {
                yaml.push('\n');
                last.column = false;
            }
            if last.kind == Kind::MappingValue {
                if sequence {
                    zero_indent = true;
                } else {
                    new_indent.push_str(INDENT);
                }
                newline = true;
            } else {
                new_indent.push_str(INDENT);
                yaml.push_str(if last.column { " " } else { &indent });
                match last.kind {
                    Kind::Sequence => yaml.push('-'),
                    Kind::Mapping => {
                        last.kind = Kind::ComplexKey;
                        zero_indent = sequence;
                        yaml.push('?');
                    }
                    Kind::ComplexValue => {
                        zero_indent = sequence;
                        yaml.push(':');
                    }
                    other => unreachable!("a collection cannot start in {other:?}"),
                }
                last.column = true;
            }
            last.newline = false;
        }
        if let Some(anchor) = anchor {
            yaml.push_str(if last.column { " " } else { &indent });
            yaml.push_str(&format!("&{anchor}"));
            newline = true;
        }
        last.index += 1;
        self.write(&yaml);
        (new_indent, newline, zero_indent)
    }

    fn push(&mut self, kind: Kind, indent: String, newline: bool, zero_indent: bool) {
        let column = self.column != 0;
        self.stack.push(Frame {
            kind,
            index: 0,
            indent,
            newline,
            column,
            zero_indent,
        });
        self.open_ended = false;
    }

    pub(super) fn mapping_start(&mut self, anchor: Option<usize>) {
        let (indent, newline, _) = self.collection_start(anchor, false);
        self.push(Kind::Mapping, indent, newline, false);
    }

    pub(super) fn sequence_start(&mut self, anchor: Option<usize>) {
        let (indent, newline, zero_indent) = self.collection_start(anchor, true);
        self.push(Kind::Sequence, indent, newline, zero_indent);
    }

    fn collection_end(&mut self, empty: &str) {
        let frame = self.stack.pop().expect("a collection was started");
        if frame.index == 0 {
            let mut indent = frame.indent;
            if frame.zero_indent {
                indent.push_str(INDENT);
            }
            let lead = if self.column != 0 { " " } else { &indent };
            self.write(&format!("{lead}{empty}\n"));
        }
        let column = self.column != 0;
        let last = self.last();
        last.column = column;
        last.kind = match last.kind {
            Kind::Mapping => Kind::MappingValue,
            Kind::MappingValue | Kind::ComplexValue => Kind::Mapping,
            Kind::ComplexKey => Kind::ComplexValue,
            other => other,
        };
    }

    pub(super) fn mapping_end(&mut self) {
        self.collection_end("{}");
    }

    pub(super) fn sequence_end(&mut self) {
        self.collection_end("[]");
    }

    pub(super) fn alias(&mut self, number: usize) {
        let mut yaml = String::new();
        let last = self
            .stack
            .last_mut()
            .expect("events arrive inside a document");
        if matches!(last.kind, Kind::Mapping | Kind::Sequence) && last.index == 0 && last.newline {
            yaml.push('\n');
            last.column = false;
            last.newline = false;
        }
        yaml.push_str(if last.column { " " } else { &last.indent });
        if last.kind == Kind::Mapping {
            yaml.push_str(&format!("*{number} :"));
            last.kind = Kind::MappingValue;
        } else {
            match last.kind {
                Kind::MappingValue => last.kind = Kind::Mapping,
                Kind::Document => {}
                Kind::ComplexValue => {
                    last.kind = Kind::Mapping;
                    yaml.push_str(": ");
                }
                Kind::ComplexKey => yaml.push_str(": "),
                Kind::Sequence => yaml.push_str("- "),
                Kind::Mapping => unreachable!("handled above"),
            }
            yaml.push_str(&format!("*{number}\n"));
        }
        last.index += 1;
        self.write(&yaml);
        let column = self.column != 0;
        self.last().column = column;
        self.open_ended = false;
    }

    pub(super) fn scalar(&mut self, value: &str, requested: Option<Style>) {
        let style = best_style(value, requested);
        let indent = self.last().indent.clone();
        let mut open_ended = false;
        let text = match style {
            Style::Plain => value.to_owned(),
            Style::Single => single_quoted(value, &format!("{indent}{INDENT}")),
            Style::Literal => {
                let (text, ended) = literal(value, &format!("{indent}{INDENT}"));
                open_ended = ended;
                text
            }
            Style::Double => double_quoted(value),
        };
        let yaml = self.block_scalar(&text, style, &indent);
        let last = self.last();
        last.index += 1;
        last.newline = false;
        self.write(&yaml);
        let column = self.column != 0;
        self.last().column = column;
        self.open_ended = open_ended;
    }

    /// `_emit_block_scalar`: places a scalar as a key, a value or a sequence entry.
    fn block_scalar(&mut self, value: &str, style: Style, indent: &str) -> String {
        let mut yaml = String::new();
        let mut text = value.to_owned();
        let multiline = style == Style::Literal;
        let last = self
            .stack
            .last_mut()
            .expect("events arrive inside a document");
        if matches!(last.kind, Kind::Mapping | Kind::Sequence) && last.index == 0 && last.newline {
            yaml.push('\n');
            last.column = false;
            last.newline = false;
        }
        if last.kind == Kind::Mapping {
            yaml.push_str(if last.column { " " } else { indent });
            last.kind = Kind::MappingValue;
            if multiline {
                // A multi-line key is written as a complex key.
                yaml.push_str("? ");
                last.kind = Kind::ComplexValue;
            } else {
                text.push(':');
            }
        } else {
            match last.kind {
                Kind::MappingValue => last.kind = Kind::Mapping,
                Kind::Document => {}
                kind => {
                    yaml.push_str(if last.column { " " } else { indent });
                    if kind == Kind::ComplexValue {
                        last.kind = Kind::Mapping;
                        yaml.push(':');
                    } else if kind == Kind::Sequence {
                        yaml.push('-');
                    } else {
                        unreachable!("a scalar cannot be written in {kind:?}");
                    }
                    last.column = true;
                }
            }
            if !text.is_empty() && last.column {
                text.insert(0, ' ');
            }
            if !multiline {
                text.push('\n');
            }
        }
        yaml.push_str(&text);
        yaml
    }
}
