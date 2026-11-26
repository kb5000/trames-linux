#version 450
// SPDX-License-Identifier: GPL-3.0-or-later
layout(push_constant) uniform Push {vec4 rect;vec4 source;vec4 flags;} p;
layout(set=0,binding=0) uniform sampler2D image;
layout(location=0) in vec2 uv;
layout(location=0) out vec4 color;
vec3 decode(vec3 c){return mix(c/12.92,pow((c+.055)/1.055,vec3(2.4)),greaterThan(c,vec3(.04045)));}
vec3 encode(vec3 c){return mix(c*12.92,1.055*pow(c,vec3(1./2.4))-.055,greaterThan(c,vec3(.0031308)));}
void main(){vec2 t=p.source.xy+uv*p.source.zw;uint transform=uint(p.flags.x);if((transform&4)!=0)t.x=1-t.x;switch(transform&3){case 1:t=vec2(t.y,1-t.x);break;case 2:t=1-t;break;case 3:t=vec2(1-t.y,t.x);break;}
color=texture(image,t);if(p.flags.y!=0)color.a=1;if(color.a>0&&p.flags.z!=p.flags.w){vec3 c=color.rgb/color.a; color.rgb=(p.flags.z!=0?decode(c):encode(c))*color.a;}}
