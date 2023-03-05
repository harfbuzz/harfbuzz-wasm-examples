BASE_FONT=../base-fonts/ArefRuqaa-Regular.otf
TARGET_FONT=ArefRuqaa-Wasm.ttf
WASM=balancer_bg.wasm

$(TARGET_FONT): pkg/$(WASM) $(BASE_FONT)
	../bin/otfsurgeon -i $(BASE_FONT) add -o $(TARGET_FONT) Wasm < pkg/$(WASM)

pkg/$(WASM): src/lib.rs
	wasm-pack build --target web
