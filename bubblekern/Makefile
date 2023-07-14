BASE_FONT=master_ttf/BubblekernSerif-Regular.ttf
TARGET_FONT=BubblekernSerif-Regular.ttf
WASM=bubblekern_bg.wasm

$(TARGET_FONT): pkg/$(WASM) $(BASE_FONT)
	../bin/otfsurgeon -i $(BASE_FONT) add -o $(TARGET_FONT) Wasm < pkg/$(WASM)

$(BASE_FONT): Bubblekern.glyphs
	fontmake -o ttf -g Bubblekern.glyphs

pkg/$(WASM): src/lib.rs
	wasm-pack build --target web
