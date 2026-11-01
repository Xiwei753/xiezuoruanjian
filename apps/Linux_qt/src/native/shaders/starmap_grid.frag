#version 440

layout(location = 0) in vec2 vScreen;

layout(location = 0) out vec4 fragColor;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec4 uColor;
    vec2 uViewport;
    float uSpacing;
    vec2 uPhase;
} ubuf;

void main()
{
    vec2 rel = mod(vScreen - ubuf.uPhase + 0.5 * ubuf.uSpacing,
                   ubuf.uSpacing) - 0.5 * ubuf.uSpacing;
    float alpha = 1.0 - smoothstep(0.75, 1.35, length(rel));
    if (alpha <= 0.0)
        discard;
    fragColor = vec4(ubuf.uColor.rgb, ubuf.uColor.a * alpha) * ubuf.qt_Opacity;
}
