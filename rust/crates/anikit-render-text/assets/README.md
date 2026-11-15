DejaVu Sans Mono, version 2.37

Source: https://github.com/dejavu-fonts/dejavu-fonts, release version_2_37
File: ttf/DejaVuSansMono.ttf, unmodified
Size: 340712 bytes
SHA-256: b4a6c3e4faab8773f4ff761d56451646409f29abedd68f05d38c2df667d3c582

LICENSE-DejaVu.txt is the licence text from the same release, kept verbatim.
It is the only tracked file in this crate that is not plain ASCII.

The face is embedded with include_bytes! in src/font.rs and is the default for
the text route. --font or ANIM_GIF_FONT replaces it, and no system font is
searched (09, row 11).
