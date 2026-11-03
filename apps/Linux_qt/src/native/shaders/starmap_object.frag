#version 440

layout(location = 0) out vec4 fragColor;

layout(location = 0) in vec2 vLocal;
layout(location = 1) in vec2 vHalf;
layout(location = 2) in float vRadius;
layout(location = 3) in float vAlpha;

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

float roundedBoxDistance(vec2 p, vec2 halfSize, float radius)
{
    vec2 q = abs(p) - halfSize + radius;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
}

void main()
{
    // 形状距离使用原始对象半宽 / 圆角；视口只影响像素外扩，不参与几何事实。
    float distancePx = roundedBoxDistance(vLocal, vHalf, vRadius)
                       * max(ubuf.uZoom, 0.000001);
    float aa = max(fwidth(distancePx), 0.0001);
    float outer = 1.0 - smoothstep(-aa, aa, distancePx);
    float inner = 1.0 - smoothstep(-aa, aa, distancePx + ubuf.uBorderPx);
    float borderMask = clamp(outer - inner, 0.0, 1.0);
    float borderMix = outer > 0.0001 ? borderMask / outer : 0.0;
    vec4 color = mix(ubuf.uFillColor, ubuf.uBorderColor, borderMix);
    fragColor = color * (ubuf.qt_Opacity * vAlpha * outer);
}
