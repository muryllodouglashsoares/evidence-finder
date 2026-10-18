import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";

interface PandaViewerProps {
  /** Tamanho fixo em px. Sem ele, o painel preenche o contêiner pai e acompanha o redimensionamento. */
  size?: number;
  className?: string;
}

const FOV = 38;
/** Fração da área visível que o panda ocupa (o restante é respiro). */
const FILL = 0.9;
/** Clipes do próprio GLB tocados uma vez, de tempos em tempos e ao clicar; depois volta ao "Idle". */
const ONE_SHOTS = ["Wave", "Yes", "No", "Punch", "Duck", "HitReact", "Sword"];

export function PandaViewer({ size, className = "" }: PandaViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [loaded, setLoaded] = useState(false);
  const [hasError, setHasError] = useState(false);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let animId: number | null = null;
    let model: THREE.Group | null = null;
    let mixer: THREE.AnimationMixer | null = null;
    let pivot: THREE.Group | null = null;
    let idleAction: THREE.AnimationAction | null = null;
    const oneShots = new Map<string, THREE.AnimationAction>();
    let current: THREE.AnimationAction | null = null;
    let lastShot = "";
    let shotTimer: number | undefined;
    let isDisposed = false;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    // Dimensões do modelo já normalizado, usadas para enquadrar a câmera.
    let halfHeight = 1;
    let horizontalRadius = 1;

    // 1. Cena e câmera (a posição é calculada em fit(), a partir do tamanho real do painel)
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(FOV, 1, 0.1, 100);

    // 2. Renderizador com fundo transparente
    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({
        antialias: true,
        alpha: true,
        powerPreference: "low-power",
      });
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
      renderer.outputColorSpace = THREE.SRGBColorSpace;
      // Evita estourar o branco e preserva as cores da textura.
      renderer.toneMapping = THREE.NeutralToneMapping;
      container.appendChild(renderer.domElement);
    } catch {
      setHasError(true);
      return;
    }

    /**
     * Enquadra o modelo: a distância da câmera é a menor que faz caber a altura E a
     * largura máxima (o panda gira, então vale o raio horizontal), com FILL de ocupação.
     * Assim o panda preenche o painel em qualquer proporção, sem recortes.
     */
    const fit = () => {
      const width = container.clientWidth;
      const height = container.clientHeight;
      if (width === 0 || height === 0) return;

      renderer.setSize(width, height, false);
      camera.aspect = width / height;

      const tanHalfFov = Math.tan(THREE.MathUtils.degToRad(FOV / 2));
      const byHeight = halfHeight / FILL / tanHalfFov;
      const byWidth = horizontalRadius / FILL / (tanHalfFov * camera.aspect);
      const distance = Math.max(byHeight, byWidth) + horizontalRadius;

      camera.position.set(0, distance * 0.1, distance);
      camera.lookAt(0, 0, 0);
      camera.updateProjectionMatrix();
    };

    // 3. Iluminação moderada: com a textura ativa, luz demais lava as cores.
    scene.add(new THREE.HemisphereLight(0xffffff, 0x8a8f9c, 1.1));

    const dirLight = new THREE.DirectionalLight(0xfff4e0, 2.0);
    dirLight.position.set(2, 4, 3);
    scene.add(dirLight);

    const rimLight = new THREE.DirectionalLight(0xd99b26, 0.9);
    rimLight.position.set(-3, 2, -2);
    scene.add(rimLight);

    /** Toca um clipe aleatório (diferente do anterior) com transição suave de e para o Idle. */
    const playOneShot = () => {
      if (!idleAction || current || reduceMotion || document.hidden) return;
      const names = [...oneShots.keys()].filter((name) => name !== lastShot);
      const name = names[Math.floor(Math.random() * names.length)];
      const action = name ? oneShots.get(name) : undefined;
      if (!name || !action) return;
      lastShot = name;
      current = action;
      action.reset().play();
      action.crossFadeFrom(idleAction, 0.25, false);
    };
    const handleFinished = (event: { action: THREE.AnimationAction }) => {
      if (event.action !== current || !idleAction) return;
      const finished = current;
      current = null;
      idleAction.reset().play();
      idleAction.crossFadeFrom(finished, 0.25, false);
    };
    const scheduleShot = () => {
      shotTimer = window.setTimeout(() => {
        playOneShot();
        scheduleShot();
      }, 5000 + Math.random() * 4000);
    };

    // 4. Modelo .glb local (public/panda.glb, sem rede)
    const loader = new GLTFLoader();
    loader.load(
      "/panda.glb",
      (gltf) => {
        if (isDisposed) return;
        model = gltf.scene;

        // Materiais: sem mapa de ambiente, metalicidade só escurece o modelo.
        model.traverse((object) => {
          if (object instanceof THREE.Mesh) {
            object.frustumCulled = false;
            const materials = Array.isArray(object.material) ? object.material : [object.material];
            materials.forEach((material: THREE.Material) => {
              if (material instanceof THREE.MeshStandardMaterial) material.metalness = 0;
            });
          }
        });

        // Animações do próprio GLB: "Idle" em laço e clipes avulsos intercalados.
        if (gltf.animations.length > 0) {
          mixer = new THREE.AnimationMixer(model);
          const find = (name: string) =>
            gltf.animations.find((clip) => clip.name.split("|").pop() === name);
          const idleClip = find("Idle") ?? gltf.animations[0];
          if (idleClip) {
            idleAction = mixer.clipAction(idleClip);
            idleAction.play();
          }
          for (const name of ONE_SHOTS) {
            const clip = find(name);
            if (!clip) continue;
            const action = mixer.clipAction(clip);
            action.setLoop(THREE.LoopOnce, 1);
            action.clampWhenFinished = false;
            oneShots.set(name, action);
          }
          mixer.addEventListener("finished", handleFinished);
          // Sem movimento contínuo: congela na primeira pose do Idle.
          mixer.update(0);
          if (reduceMotion) mixer.timeScale = 0;
          else scheduleShot();
        }
        model.updateMatrixWorld(true);

        // Normaliza: maior dimensão = 2 unidades, centrado na origem (já na pose do Idle).
        const box = new THREE.Box3().setFromObject(model);
        const center = box.getCenter(new THREE.Vector3());
        const boxSize = box.getSize(new THREE.Vector3());
        const scale = 2 / (Math.max(boxSize.x, boxSize.y, boxSize.z) || 1);

        model.scale.setScalar(scale);
        model.position.set(-center.x * scale, -center.y * scale, -center.z * scale);

        halfHeight = (boxSize.y * scale) / 2;
        horizontalRadius = Math.hypot(boxSize.x, boxSize.z) * scale / 2;

        // O pivô gira em torno do centro do panda (o modelo já está centrado dentro dele).
        pivot = new THREE.Group();
        pivot.add(model);
        scene.add(pivot);
        fit();
        setLoaded(true);
      },
      undefined,
      () => {
        if (!isDisposed) setHasError(true);
      },
    );

    const resizeObserver = new ResizeObserver(fit);
    resizeObserver.observe(container);
    fit();

    // 5. Cursor: o panda se vira para onde o mouse estiver, em qualquer ponto da janela
    let targetYaw = 0;
    let targetPitch = 0;
    const handlePointerMove = (e: PointerEvent) => {
      const rect = container.getBoundingClientRect();
      const dx = (e.clientX - (rect.left + rect.width / 2)) / (window.innerWidth / 2);
      const dy = (e.clientY - (rect.top + rect.height / 2)) / (window.innerHeight / 2);
      targetYaw = THREE.MathUtils.clamp(dx, -1, 1) * 0.9;
      targetPitch = THREE.MathUtils.clamp(dy, -1, 1) * 0.18;
    };
    const handlePointerLeave = () => {
      targetYaw = 0;
      targetPitch = 0;
    };
    window.addEventListener("pointermove", handlePointerMove);
    document.documentElement.addEventListener("pointerleave", handlePointerLeave);
    container.addEventListener("click", playOneShot);

    // 6. Animação; pausa com a janela oculta
    const timer = new THREE.Timer();
    const animate = () => {
      animId = requestAnimationFrame(animate);
      timer.update();
      const delta = Math.min(timer.getDelta(), 0.1);

      mixer?.update(delta);
      if (pivot) {
        const k = 1 - Math.exp(-8 * delta);
        pivot.rotation.y += (targetYaw - pivot.rotation.y) * k;
        pivot.rotation.x += (targetPitch - pivot.rotation.x) * k;
      }

      renderer.render(scene, camera);
    };

    const handleVisibility = () => {
      if (document.hidden) {
        if (animId !== null) cancelAnimationFrame(animId);
        animId = null;
      } else if (animId === null && !isDisposed) {
        animate();
      }
    };
    document.addEventListener("visibilitychange", handleVisibility);

    animate();

    return () => {
      isDisposed = true;
      if (animId !== null) cancelAnimationFrame(animId);
      resizeObserver.disconnect();
      document.removeEventListener("visibilitychange", handleVisibility);
      window.removeEventListener("pointermove", handlePointerMove);
      document.documentElement.removeEventListener("pointerleave", handlePointerLeave);
      container.removeEventListener("click", playOneShot);
      window.clearTimeout(shotTimer);
      mixer?.stopAllAction();

      scene.traverse((object) => {
        if (object instanceof THREE.Mesh) {
          object.geometry.dispose();
          const materials = Array.isArray(object.material) ? object.material : [object.material];
          materials.forEach((material: THREE.Material) => material.dispose());
        }
      });
      scene.clear();
      renderer.dispose();
      if (renderer.domElement.parentElement) {
        renderer.domElement.parentElement.removeChild(renderer.domElement);
      }
    };
  }, []);

  const fixed = size !== undefined;
  const boxStyle = fixed ? { width: `${size}px`, height: `${size}px` } : undefined;
  const placeholderSize = fixed ? Math.floor(size * 0.4) : 96;

  if (hasError) {
    return (
      <div
        className={`panda panda--fallback ${fixed ? "" : "panda--fill"} ${className}`}
        style={boxStyle}
      >
        <span style={{ fontSize: `${placeholderSize}px` }}>🐼</span>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className={`panda ${fixed ? "" : "panda--fill"} ${className}`}
      style={boxStyle}
    >
      {!loaded && (
        <span className="panda__placeholder" style={{ fontSize: `${placeholderSize}px` }}>
          🐼
        </span>
      )}
    </div>
  );
}
