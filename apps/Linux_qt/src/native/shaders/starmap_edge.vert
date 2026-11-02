#version 440

layout(location = 0) in vec2 aPos;
layout(location = 1) in vec2 aDir;
layout(location = 2) in vec2 aOffset;
layout(location = 3) in float aKind;

layout(location = 0) out float vEdgeDistance;
layout(location = 1) out float vSolid;

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
        // 顶点扩到半线宽 + 0.5px；vEdgeDistance 也使用同一个半宽 + 0.5px，
        // 这样最外缘正好是 0 覆盖率，片元不会在几何边缘突然截断。
        float outerHalf = ubuf.uStrokeHalf + 0.5;
        offsetPx = normal * (aOffset.y * outerHalf);
        vEdgeDistance = aOffset.y * outerHalf;
        vSolid = 0.0;
    } else {
        // 箭头保持实心三角形，不参与线宽覆盖率。
        float screenPerWorld = length(clipDir);
        offsetPx = (dir * aOffset.x + normal * aOffset.y) * screenPerWorld;
        vEdgeDistance = 0.0;
        vSolid = 1.0;
    }

    gl_Position = center;
    gl_Position.xy += (offsetPx / (ubuf.uViewport * 0.5)) * center.w;
}
