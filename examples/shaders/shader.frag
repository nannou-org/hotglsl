#version 450

layout(location = 0) in vec2 tex_coords;
layout(location = 0) out vec4 f_color;

void main() {
    f_color = vec4(tex_coords, 0.0, 1.0);
}
