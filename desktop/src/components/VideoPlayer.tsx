import React, { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import { listen } from "@tauri-apps/api/event";

const VideoPlayer: React.FC = () => {
  console.log("VideoPlayer");
  const videoRef = useRef<HTMLVideoElement>(null);

  useEffect(() => {
    const mediaSource = new MediaSource();
    let sourceBuffer: SourceBuffer;

    if (videoRef.current) {
      videoRef.current.src = URL.createObjectURL(mediaSource);
    }

    mediaSource.addEventListener("sourceopen", () => {
      sourceBuffer = mediaSource.addSourceBuffer(
        'video/mp4; codecs="avc1.42E01E, mp4a.40.2"'
      );
    });

    const handleVideoChunk = (event: any) => {
      const chunk = new Uint8Array(event.payload);
      if (sourceBuffer.updating) {
        console.log("sourceBuffer updating");
        sourceBuffer.addEventListener("updateend", () => {
          sourceBuffer.appendBuffer(chunk);
        });
      } else {
        sourceBuffer.appendBuffer(chunk);
      }
    };

    const unlisten = listen("video-chunk", handleVideoChunk);
    invoke("download_s3_video");

    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  return <video ref={videoRef} controls />;
};

export default VideoPlayer;
