BASE_FONT=../base-fonts/NotoSansEgyptianHieroglyphs-Regular.ttf
TARGET_FONT=Hieroglyphs-Regular.ttf
WASM=hieroglyphs_bg.wasm

$(TARGET_FONT): pkg/$(WASM) $(BASE_FONT)
	../bin/otfsurgeon -i $(BASE_FONT) add -o $(TARGET_FONT) Wasm < pkg/$(WASM)

pkg/$(WASM): src/lib.rs
	wasm-pack build --target web
