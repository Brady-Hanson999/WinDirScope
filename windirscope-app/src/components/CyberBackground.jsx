import React, { useRef, useEffect } from "react";

export default function CyberBackground({
  speed = 1,
  density = 1,
  brightness = 1,
  particleChance = 0.005,
}) {
  const canvasRef = useRef(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    let width, height;
    let animationFrameId;
    let traces = [];
    let particles = [];

    const colorDefs = [
      { r: 0, g: 255, b: 255, baseA: 0.2 }, // cyan
      { r: 100, g: 150, b: 255, baseA: 0.2 }, // soft blue
      { r: 180, g: 100, b: 255, baseA: 0.15 }, // faint purple
    ];

    const resize = () => {
      width = canvas.width = window.innerWidth;
      height = canvas.height = window.innerHeight;
      
      // Keep traces when resizing to avoid visual jump, but ensure they don't break
      if (traces.length === 0) {
        initTraces();
      }
    };

    window.addEventListener("resize", resize);

    class Trace {
      constructor(isRespawn = false) {
        this.reset(isRespawn);
        if (!isRespawn) {
          // Initial spawn anywhere on screen
          this.x = Math.random() * width;
          this.y = Math.random() * height;
          // Pre-fill history to avoid all traces looking like they just started
          for (let i = 0; i < this.maxLength / 2; i++) {
            this.history.push({
              x: this.x - this.vx * i,
              y: this.y - this.vy * i,
            });
          }
        }
      }

      reset(fromLeft = true) {
        this.colorDef = colorDefs[Math.floor(Math.random() * colorDefs.length)];
        this.color = `rgba(${this.colorDef.r}, ${this.colorDef.g}, ${
          this.colorDef.b
        }, ${this.colorDef.baseA * brightness})`;
        this.width = Math.random() * 1.5 + 0.5;
        this.maxLength = Math.floor(Math.random() * 120 + 40);
        this.history = [];
        this.pulse = Math.random() > 0.87 ? { active: false, index: 0 } : null;

        // Spawn left side, or randomly if not fromLeft
        this.x = fromLeft ? -50 : Math.random() * width;
        this.y = Math.random() * height;

        // Base velocity (moving right, slight drift y)
        this.vx = (Math.random() * 1.2 + 0.4) * speed;
        this.vy = (Math.random() - 0.5) * speed;

        this.turnTimer = Math.floor(Math.random() * 80 + 40);
        this.dead = false;
      }

      update() {
        if (this.dead) return;

        // "Circuit" style turning: occasionally snap velocity dy to 45 deg angles
        this.turnTimer--;
        if (this.turnTimer <= 0) {
          this.turnTimer = Math.floor(Math.random() * 100 + 50);
          // Snap Y velocity slightly
          const driftDir = Math.random() > 0.5 ? 1 : -1;
          this.vy = driftDir * (Math.random() * 0.4) * speed;
        }

        this.x += this.vx;
        this.y += this.vy;

        this.history.unshift({ x: this.x, y: this.y });
        if (this.history.length > this.maxLength) {
          this.history.pop();
        }

        // Data pulse logic
        if (this.pulse) {
          if (!this.pulse.active && Math.random() < 0.0016 * speed) {
            this.pulse.active = true;
            this.pulse.index = this.history.length - 1; // start at tail
          }
          if (this.pulse.active) {
            this.pulse.index -= 1 * speed; // move to head (half speed)
            if (this.pulse.index <= 0) {
              this.pulse.active = false;
            }
          }
        }

        // Particles
        if (Math.random() < particleChance) {
          particles.push(new Particle(this.x, this.y, this.colorDef));
        }

        // Garbage collection
        if (this.x > width + 100 || this.y < -100 || this.y > height + 100) {
          this.dead = true;
        }
      }

      draw(ctx) {
        if (this.history.length < 2) return;

        ctx.beginPath();
        ctx.moveTo(this.history[0].x, this.history[0].y);
        for (let i = 1; i < this.history.length; i++) {
          ctx.lineTo(this.history[i].x, this.history[i].y);
        }

        ctx.strokeStyle = this.color;
        ctx.lineWidth = this.width;
        ctx.shadowBlur = 8 * brightness;
        ctx.shadowColor = `rgba(${this.colorDef.r}, ${this.colorDef.g}, ${this.colorDef.b}, 1)`;
        ctx.stroke();
        ctx.shadowBlur = 0;

        // Draw pulse
        if (this.pulse && this.pulse.active) {
          const pIndex = Math.floor(this.pulse.index);
          if (pIndex >= 0 && pIndex < this.history.length) {
            const p = this.history[pIndex];
            ctx.beginPath();
            ctx.arc(p.x, p.y, this.width * 2, 0, Math.PI * 2);
            ctx.fillStyle = "rgba(255, 255, 255, 0.75)";
            ctx.shadowBlur = 8 * brightness;
            ctx.shadowColor = "rgba(255, 255, 255, 0.6)";
            ctx.fill();
            ctx.shadowBlur = 0;
          }
        }
      }
    }

    class Particle {
      constructor(x, y, colorDef) {
        this.x = x;
        this.y = y;
        this.colorDef = colorDef;
        this.vx = (Math.random() - 0.5) * 1.5;
        this.vy = (Math.random() - 0.5) * 1.5;
        this.life = 1;
        this.decay = Math.random() * 0.02 + 0.01;
      }
      update() {
        this.x += this.vx;
        this.y += this.vy;
        this.life -= this.decay;
      }
      draw(ctx) {
        if (this.life <= 0) return;
        ctx.beginPath();
        ctx.arc(this.x, this.y, Math.random() * 1.5, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${this.colorDef.r}, ${this.colorDef.g}, ${
          this.colorDef.b
        }, ${this.life * this.colorDef.baseA * brightness})`;
        ctx.fill();
      }
    }

    let mouseX = 0;
    let mouseY = 0;
    let targetMouseX = 0;
    let targetMouseY = 0;

    const handleMouseMove = (e) => {
      if (width && height) {
        targetMouseX = (e.clientX / width - 0.5) * 20; // max offset
        targetMouseY = (e.clientY / height - 0.5) * 20;
      }
    };
    window.addEventListener("mousemove", handleMouseMove);

    const initTraces = () => {
      traces = [];
      const numTraces = Math.floor(35 * density);
      for (let i = 0; i < numTraces; i++) {
        traces.push(new Trace(false));
      }
    };

    const animate = () => {
      ctx.clearRect(0, 0, width, height);

      // Interpolate mouse parallax
      mouseX += (targetMouseX - mouseX) * 0.05;
      mouseY += (targetMouseY - mouseY) * 0.05;

      ctx.save();
      ctx.translate(mouseX, mouseY);

      // Traces
      for (let i = 0; i < traces.length; i++) {
        if (traces[i].dead) {
          traces[i] = new Trace(true);
        }
        traces[i].update();
        traces[i].draw(ctx);
      }

      // Particles
      for (let i = particles.length - 1; i >= 0; i--) {
        particles[i].update();
        if (particles[i].life <= 0) {
          particles.splice(i, 1);
        } else {
          particles[i].draw(ctx);
        }
      }

      ctx.restore();

      animationFrameId = requestAnimationFrame(animate);
    };

    resize();
    initTraces();
    animate();

    return () => {
      window.removeEventListener("resize", resize);
      window.removeEventListener("mousemove", handleMouseMove);
      cancelAnimationFrame(animationFrameId);
    };
  }, [speed, density, brightness, particleChance]);

  return (
    <canvas
      ref={canvasRef}
      style={{
        position: "absolute",
        top: 0,
        left: 0,
        width: "100%",
        height: "100%",
        zIndex: -1,
        pointerEvents: "none",
        background: "transparent",
      }}
    />
  );
}
