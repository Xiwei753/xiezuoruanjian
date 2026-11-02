#version 440

layout(location = 0) out vec4 fragColor;
layout(location = 0) in float vEdgeDistance;
layout(location = 1) in float vSolid;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec4 uColor;
    vec2 uViewport;
    float uStrokeHalf;
} ubuf;

void main()
{
    // 线段：vEdgeDistance 是从线心到几何外缘的屏幕像素距离；箭头：实心。
    float coverage = vSolid > 0.5
        ? 1.0
        : clamp(ubuf.uStrokeHalf + 0.5 - abs(vEdgeDistance), 0.0, 1.0);
    fragColor = ubuf.uColor * (ubuf.qt_Opacity * coverage);
}
