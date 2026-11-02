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
    // aPos 是相对稳定 world anchor 的局部坐标，qt_Matrix 负责 anchor 平移
    // 与 zoom 缩放；线宽和箭头仍在屏幕像素空间展开。
    vec4 center = ubuf.qt_Matrix * vec4(aPos, 0.0, 1.0);
    vec4 ahead = ubuf.qt_Matrix * vec4(aPos + aDir, 0.0, 1.0);
    vec2 clipDir = (ahead.xy / ahead.w - center.xy / center.w)
                   * ubuf.uViewport * 0.5;
    vec2 dir = length(clipDir) > 1e-5 ? normalize(clipDir) : vec2(0.0, -1.0);
    vec2 normal = vec2(-dir.y, dir.x);

    vec2 offsetPx;
    if (aKind < 0.5) {
        // 几何外扩到半线宽 + 0.5px，vEdgeDistance 记录真实屏幕像素距离；
        // 片元覆盖率在几何外缘平滑降到 0。
        float outerHalf = ubuf.uStrokeHalf + 0.5;
        offsetPx = normal * (aOffset.y * outerHalf);
        vEdgeDistance = aOffset.y * outerHalf;
        vSolid = 0.0;
    } else {
        // 箭头顶点都落在真实箭尖；aOffset 是屏幕像素，不随 zoom 放大。
        offsetPx = dir * aOffset.x + normal * aOffset.y;
        vEdgeDistance = 0.0;
        vSolid = 1.0;
    }

    gl_Position = center;
    gl_Position.xy += (offsetPx / (ubuf.uViewport * 0.5)) * center.w;
}
