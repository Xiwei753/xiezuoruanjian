#version 440

layout(location = 0) out vec4 fragColor;
layout(location = 0) in float vEdgeDistance;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec4 uColor;
    vec2 uViewport;
    float uStrokeHalf;
} ubuf;

void main()
{
    float coverage = clamp(ubuf.uStrokeHalf + 0.5 - abs(vEdgeDistance), 0.0, 1.0);
    fragColor = ubuf.uColor * (ubuf.qt_Opacity * coverage);
}
