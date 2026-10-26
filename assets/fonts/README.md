# Bundled fonts

Subsets of these fonts ship inside the app, so it looks the same on every system.

| File | Source | License |
| --- | --- | --- |
| `NotoSans-Regular.ttf`, `NotoSans-Bold.ttf` | [Noto Sans](https://github.com/notofonts/latin-greek-cyrillic) | SIL OFL 1.1, `OFL-NotoSans.txt` |
| `JetBrainsMono-Medium.ttf` | [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono), from the Nerd Fonts build | SIL OFL 1.1, `OFL-JetBrainsMono.txt` |
| `ui-icons.ttf` | the icon glyphs of JetBrainsMono Nerd Font from [Nerd Fonts](https://github.com/ryanoasis/nerd-fonts) | MIT, see `../os-icons-LICENSE.txt` |

Rebuild them with `pyftsubset` from fontTools:

    T='U+0020-007E,U+00A0-024F,U+0300-036F,U+0370-03FF,U+0400-04FF,U+1E00-1EFF,U+2000-206F,U+20A0-20CF,U+2100-215F,U+2190-21FF,U+2200-22FF,U+2300-23FF,U+25A0-25FF,U+2600-26FF,U+2700-27BF'
    pyftsubset NotoSans-Regular.ttf --unicodes="$T" --layout-features='*' --output-file=NotoSans-Regular.ttf
    pyftsubset NotoSans-Bold.ttf --unicodes="$T" --layout-features='*' --output-file=NotoSans-Bold.ttf
    pyftsubset JetBrainsMonoNerdFont-Medium.ttf --unicodes='U+0020-007E,U+00A0-017F,U+2000-206F,U+2190-21FF,U+2022,U+25CF' --layout-features='*' --output-file=JetBrainsMono-Medium.ttf

`ui-icons.ttf` takes the codepoints in `src/theme.rs`'s `icon` module, the `\u{f...}` ones outside
`src/osicon.rs`:

    pyftsubset JetBrainsMonoNerdFont-Medium.ttf --unicodes='U+F007,U+F00C,...' --output-file=ui-icons.ttf

Add a new icon's codepoint to that list when you add the icon.
