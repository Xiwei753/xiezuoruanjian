#version 440

layout(location = 0) in vec2 aPos;

layout(location = 0) out vec2 vScreen;

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
    vScreen = aPos * ubuf.uViewport;
    gl_Position = ubuf.qt_Matrix * vec4(vScreen, 0.0, 1.0);
}
