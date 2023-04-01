BASE_FONT=../base-fonts/NotoNastaliqUrdu-VF.ttf
TARGET_FONT=NotoNastaliq-Wasm.ttf
WASM=gulzar_shaper_bg.wasm

$(TARGET_FONT): pkg/$(WASM) $(BASE_FONT)
	../bin/otfsurgeon -i $(BASE_FONT) add -o $(TARGET_FONT) Wasm < pkg/$(WASM)

pkg/$(WASM): src/lib.rs
	wasm-pack build --target web
