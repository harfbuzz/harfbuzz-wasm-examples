BASE_FONT=../base-fonts/Recursive-subset.ttf
SLIM_BASE_FONT=Recursive-slim.ttf
TARGET_FONT=Inception-VF.ttf
TARGET_SOURCE=Inception.glyphs
WASM=inception_bg.wasm

# This one is a little different.
$(TARGET_FONT): pkg/$(WASM) $(SLIM_BASE_FONT) $(TARGET_SOURCE) colr.ttx
	fontmake -o variable -g $(TARGET_SOURCE)
	../bin/otfsurgeon -i variable_ttf/Inception-VF.ttf add -o $(TARGET_FONT) Font < $(SLIM_BASE_FONT)
	../bin/otfsurgeon -i $(TARGET_FONT) add -o $(TARGET_FONT) Wasm < pkg/$(WASM)
	ttx -o $(TARGET_FONT) -m $(TARGET_FONT) colr.ttx

$(SLIM_BASE_FONT): $(BASE_FONT)
	../bin/otfsurgeon -i $(BASE_FONT) strip -o $(SLIM_BASE_FONT) 'name' 'OS/2' 'MVAR' 'prep' 'HVAR' 'STAT' 'gasp'

pkg/$(WASM): src/lib.rs
	wasm-pack build --target web
