#version 450
// SPDX-License-Identifier: GPL-3.0-or-later
layout(push_constant) uniform Push {vec4 rect;vec4 source;vec4 flags;} p;
layout(location=0) out vec2 uv;
void main(){vec2 points[4]=vec2[](vec2(0,0),vec2(1,0),vec2(0,1),vec2(1,1));uv=points[gl_VertexIndex];gl_Position=vec4(p.rect.xy+uv*p.rect.zw,0,1);}
