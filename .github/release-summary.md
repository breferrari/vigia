vigia 1.2.0 shows pictures. A changed image appears as its old and new versions side by side, with the changed pixels marked: PNG, JPEG, GIF, WebP, SVG, and the first changed page of a PDF. It uses the terminal's image protocol where there is one (Kitty graphics, Sixel, iTerm2) and coloured half blocks where there isn't. Every build draws them, `cargo install vigia` included.

Pictures stay out of the way. They go to the terminal between frames, so scrolling and typing stay responsive while a large one is still arriving. Ctrl+L redraws the pane and sends its pictures again, and a font zoom redraws them at the new size. The mask colour has its own theme key, `picture_changed`.

Moved files read the way git reads them. A file that was moved and edited shows as a rename when git would call it one, text included, and a move of a few hundred similar files keeps every rename. An image moved and changed shows as a rename with both pictures, and a Git LFS file moved without an edit shows as a rename too. An LFS image redraws when its object arrives.

Markdown files with fenced code blocks highlight up to twelve times faster. The release archives are attested, and each binary carries its crate list. Building from source needs Rust 1.99 or newer.

Known limits: inside herdr, scrolling over pictures can still stutter briefly, because herdr redraws every picture on each frame. Ghostty 1.3 receives pictures uncompressed, because its decoder crashes on some compressed streams; this lifts when a Ghostty release ships its new decoder.
