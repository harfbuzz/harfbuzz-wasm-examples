BASE_FONT=../base-fonts/DrShadow-Regular.ttf
TARGET_FONT=DrShadow-Wasm.ttf
WASM=shadow_bg.wasm

$(TARGET_FONT): pkg/$(WASM) $(BASE_FONT) colr.ttx
	../bin/otfsurgeon -i $(BASE_FONT) add -o $(TARGET_FONT) Wasm < pkg/$(WASM)
	ttx -o $(TARGET_FONT) -m $(TARGET_FONT) colr.ttx

pkg/$(WASM): src/lib.rs
	wasm-pack build --target web
