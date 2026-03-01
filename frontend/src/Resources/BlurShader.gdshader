// 背景画像にガウシアンブラーをかけるシェーダー
// キャラクターの存在感を引き立てるため背景をぼかす
shader_type canvas_item;

uniform float blur_amount : hint_range(0.0, 10.0) = 3.0;

void fragment() {
    vec2 uv = UV;
    vec4 color = vec4(0.0);
    float total = 0.0;

    // 7x7 ボックスブラー（TEXTURE_PIXEL_SIZE = 1テクセルのUV幅）
    for (float x = -3.0; x <= 3.0; x += 1.0) {
        for (float y = -3.0; y <= 3.0; y += 1.0) {
            vec2 offset = vec2(x, y) * blur_amount * TEXTURE_PIXEL_SIZE;
            color += texture(TEXTURE, uv + offset);
            total += 1.0;
        }
    }

    COLOR = color / total;
}
