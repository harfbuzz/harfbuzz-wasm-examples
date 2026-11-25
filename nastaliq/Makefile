BASE_FONT=../base-fonts/NotoNastaliqUrdu-VF.ttf
TARGET_FONT=NotoNastaliq-Wasm.ttf
WASM=gulzar_shaper_bg.wasm
# What the module is built from.
SOURCES=Cargo.toml .cargo/config.toml wasm-ld-untee $(wildcard src/*.rs) \
	../harfbuzz-wasm/Cargo.toml $(wildcard ../harfbuzz-wasm/src/*.rs)

$(TARGET_FONT): pkg/$(WASM) $(BASE_FONT)
	../bin/otfsurgeon -i $(BASE_FONT) add -o $(TARGET_FONT) Wasm < pkg/$(WASM)

pkg/$(WASM): $(SOURCES)
	wasm-pack build --target web
