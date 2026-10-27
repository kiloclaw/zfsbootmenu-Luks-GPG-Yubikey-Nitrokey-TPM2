# The menu's text

Every string the menu shows comes from a [Project Fluent](https://projectfluent.org) catalogue,
`crates/zbm-tui/locales/en/menu.ftl`, never from a literal in the code (ROADMAP.md, R5 step 2). A translation
(ROADMAP.md #18: Spanish, French, Dutch, Brazilian Portuguese) is then a new catalogue, not a change of the code.
Logs and the journal stay in English and do not use it.

## Writing text

`zbm_tui::text` formats messages. In the code:

```rust
text!("pool-host", hostid = hostid.as_str())   // "host deadbeef"
text!("view-snapshots.of", environment = name)  // an attribute: "Snapshots of zroot/ROOT/void"
id!("action-boot")                              // an id, to format later
crate::text::title(id!("action-boot"))          // its title: "Boot"
```

and in the catalogue:

```fluent
pool-host = host { $hostid }
view-snapshots = Snapshots
    .of = Snapshots of { $environment }
action-boot = boot
    .title = Boot
badge-problems = { $count ->
        [one] { $count } problem
       *[other] { $count } problems
    }
```

- **Titles.** Title case (`Clone into a New Boot Environment`, the toolbar's `Boot`) is a rule of English, so a
  message that is shown both ways keeps its title form in a `.title` attribute. The toolbar and the help view look the
  title up from the id of what a key does.
- **Plurals, numbers, ages and sizes** go through the catalogue: `count` selects the plural form, `size` the binary
  unit, and `age-*` say how long ago. A number with decimals is passed with `text::decimal("1.5")`.
- **Data is not text.** Names of datasets, pools, volumes, kernels and packages, and what the machine says (check
  results, failure reasons, start-up stage names, log lines), are values in placeholders. The application layer still
  writes some of the latter in English; R5 step 3 gives them ids as the real machine feeds the menu.
- **Widths** are counted in characters, never in bytes, so accented text measures right.
- Unicode isolation marks around placeholders are off: terminals would count them as characters.

## The checks

`cargo xtask lint` ("menu text", `xtask/src/texts.rs`) fails when:

- the menu's code (`crates/zbm-tui/src/menu`, outside tests) has a string with a letter outside the placeholders of a
  format string, other than the id of `text!` or `id!`. A few files hold strings that are not text: the fake
  machine of `--demo`, terminal names, keyboard rows, device paths and the ASCII glyphs (`NOT_TEXT` in the module
  says why for each);
- an id in the code, tests included, is not in the catalogue;
- a message or attribute of the catalogue is not used. A `.title` counts as used when its message is named.

The unit tests of `zbm_tui::text` check that the catalogue parses without errors or duplicates, and those of the key
map that every action has a title.

## The pseudo-language

`Language::Pseudo` is English with every letter accented (`Boot` reads `BBóótt`) and every text a third longer, as a
translation may be. The wireframe tests draw every view at 52×20 in it
(`docs/dev/menu-design/wireframes/*-52x20-pseudo.snap`), and check that the console fonts can draw it. Text that is
not from the catalogue shows unaccented, and text that does not fit shows cut or misaligned: step 7 of R5 makes every
view fit at 52×20 in it. Try it with:

```sh
cargo run -p zbm-tui -- --demo --language pseudo
```

## Adding a language

1. Copy `locales/en/menu.ftl` to `locales/<language>/menu.ftl` and translate every message, keeping the ids and the
   placeholders. Use the language's own plural categories (`[one]`, `[few]`, `[many]` and `*[other]`) and its own
   title forms.
2. Add the language to `Language` in `crates/zbm-tui/src/text.rs`, with its catalogue and its language identifier.
3. Check the wireframes in it at 52×20, as for the pseudo-language.

The characters must be in the 256-glyph console fonts the image installs: Latin-1 and the few letters beyond it that
those four languages use (`zbm_types::psf::repertoire`).
