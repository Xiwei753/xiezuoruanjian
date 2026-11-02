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
    // 顶点已经在 CPU 侧按当前相机投影成视口局部像素坐标。这里只做
    // 固定屏幕像素的线宽外扩，不再把世界方向交给矩阵缩放。
    vec2 dir = length(aDir) > 1e-5 ? normalize(aDir) : vec2(0.0, -1.0);
    vec2 normal = vec2(-dir.y, dir.x);

    vec2 offsetPx;
    if (aKind < 0.5) {
        // 几何扩到半线宽 + 0.5px；片元按到线心的真实像素距离衰减，
        // 亚像素线宽和抗锯齿都走同一条覆盖规则。
        float outerHalf = ubuf.uStrokeHalf + 0.5;
        offsetPx = normal * (aOffset.y * outerHalf);
        vEdgeDistance = aOffset.y * outerHalf;
        vSolid = 0.0;
    } else {
        // 箭头三角形固定屏幕尺寸，保持实心。
        offsetPx = aOffset;
        vEdgeDistance = 0.0;
        vSolid = 1.0;
    }

    gl_Position = ubuf.qt_Matrix * vec4(aPos + offsetPx, 0.0, 1.0);
}
