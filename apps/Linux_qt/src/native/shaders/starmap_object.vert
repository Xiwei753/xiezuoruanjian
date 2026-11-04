#version 440

layout(location = 0) in vec2 aCenter;
layout(location = 1) in vec2 aHalf;
layout(location = 2) in vec2 aCorner;
layout(location = 3) in float aRadius;
layout(location = 4) in float aAlpha;

layout(location = 0) out vec2 vLocal;
layout(location = 1) out vec2 vHalf;
layout(location = 2) out float vRadius;
layout(location = 3) out float vAlpha;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec4 uFillColor;
    vec4 uBorderColor;
    vec2 uViewport;
    float uBorderPx;
    float uZoom;
    float uReveal;
    float uDotWorldRadius;
} ubuf;

void main()
{
    // 对象几何始终是完整 world 尺寸；展开进度只改 shader 里的有效半宽，
    // 相机只改 qt_Matrix，二者都不重建顶点。
    float zoom = max(ubuf.uZoom, 0.000001);
    vec2 halfSize = mix(vec2(max(ubuf.uDotWorldRadius, 0.0)), aHalf,
                        clamp(ubuf.uReveal, 0.0, 1.0));
    float maxRadius = min(halfSize.x, halfSize.y);
    float radius = mix(maxRadius, min(aRadius, maxRadius),
                       clamp(ubuf.uReveal, 0.0, 1.0));

    // 角点与中心必须经过同一份完整模型-视图-投影矩阵；不能再用当前 Item 的
    // 宽高把像素偏移归一化到 NDC，否则活动画布/子图预览尺寸不同时会误缩放。
    vec2 expandWorld = vec2(1.5 / zoom);
    vec2 cornerWorld = aCenter + aCorner * (halfSize + expandWorld);
    gl_Position = ubuf.qt_Matrix * vec4(cornerWorld, 0.0, 1.0);

    vLocal = aCorner * (halfSize + expandWorld);
    vHalf = halfSize;
    vRadius = radius;
    vAlpha = aAlpha;
}
