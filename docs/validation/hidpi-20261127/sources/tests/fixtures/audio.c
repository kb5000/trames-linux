// SPDX-License-Identifier: GPL-3.0-or-later
// Same original ALSA client runs natively and through the guest PCM plugin.
#include <alsa/asoundlib.h>
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include <errno.h>
#define CHECK(x) do{int e=(x);if(e<0){fprintf(stderr,"%s: %s\n",#x,snd_strerror(e));return 1;}}while(0)
int main(int argc,char **argv){
    if(argc!=3)return 2;int capture=!strcmp(argv[2],"capture");snd_pcm_t *pcm;
    CHECK(snd_pcm_open(&pcm,argv[1],capture?SND_PCM_STREAM_CAPTURE:SND_PCM_STREAM_PLAYBACK,0));
    snd_pcm_hw_params_t *hw;snd_pcm_hw_params_alloca(&hw);CHECK(snd_pcm_hw_params_any(pcm,hw));
    CHECK(snd_pcm_hw_params_set_access(pcm,hw,SND_PCM_ACCESS_RW_INTERLEAVED));CHECK(snd_pcm_hw_params_set_format(pcm,hw,SND_PCM_FORMAT_S16_LE));CHECK(snd_pcm_hw_params_set_channels(pcm,hw,2));CHECK(snd_pcm_hw_params_set_rate(pcm,hw,48000,0));CHECK(snd_pcm_hw_params_set_period_size(pcm,hw,480,0));CHECK(snd_pcm_hw_params_set_buffer_size(pcm,hw,1920));CHECK(snd_pcm_hw_params(pcm,hw));
    unsigned char samples[1920];for(unsigned i=0;i<sizeof samples;i++)samples[i]=capture?0:(i%251);
    if(capture)CHECK(snd_pcm_start(pcm));
    for(int period=0;period<2;period++){
        snd_pcm_sframes_t frames=capture?snd_pcm_readi(pcm,samples,480):snd_pcm_writei(pcm,samples,480);
        if(frames!=480){fprintf(stderr,"frames=%ld\n",(long)frames);return 3;}
        if(capture)for(unsigned i=0;i<sizeof samples;i++)if(samples[i]!=0x5a)return 4;
    }
    if(!capture)CHECK(snd_pcm_drain(pcm));CHECK(snd_pcm_drop(pcm));CHECK(snd_pcm_prepare(pcm));CHECK(snd_pcm_close(pcm));
    printf("%s:960 frames\n",capture?"capture":"playback");return 0;
}
