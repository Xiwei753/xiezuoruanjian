#version 440

layout(location = 0) in vec2 aPos;
layout(location = 1) in vec2 aDir;
layout(location = 2) in vec2 aOffset;
layout(location = 3) in float aKind;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec4 uColor;
    vec2 uViewport;
    float uStrokeHalf;
} ubuf;

void main()
{
    vec4 center = ubuf.qt_Matrix * vec4(aPos, 0.0, 1.0);
    vec4 ahead = ubuf.qt_Matrix * vec4(aPos + aDir, 0.0, 1.0);
    vec2 clipDir = (ahead.xy / ahead.w - center.xy / center.w)
                   * ubuf.uViewport * 0.5;
    vec2 dir = length(clipDir) > 1e-5 ? normalize(clipDir) : vec2(0.0, -1.0);
    vec2 normal = vec2(-dir.y, dir.x);

    vec2 offsetPx;
    if (aKind < 0.5) {
        offsetPx = normal * (aOffset.y * ubuf.uStrokeHalf);
    } else {
        float screenPerWorld = length(clipDir);
        offsetPx = (dir * aOffset.x + normal * aOffset.y) * screenPerWorld;
    }

    gl_Position = center;
    gl_Position.xy += (offsetPx / (ubuf.uViewport * 0.5)) * center.w;
}
