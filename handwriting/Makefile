BASE_FONT=variable_ttf/Hand-Dot-VF.ttf
TARGET_FONT=Hand-Wasm.ttf
WASM=hand_bg.wasm

$(TARGET_FONT): pkg/$(WASM) $(BASE_FONT)
	../bin/otfsurgeon -i $(BASE_FONT) add -o $(TARGET_FONT) Wasm < pkg/$(WASM)
	ttx -o $(TARGET_FONT) -m $(TARGET_FONT) fvar.ttx

$(BASE_FONT): Hand.glyphs
	fontmake -o variable -g Hand.glyphs --keep-overlaps --keep-direction

pkg/$(WASM): src/lib.rs
	wasm-pack build --target web
