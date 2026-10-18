import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";

interface PandaViewerProps {
  size?: number;
  className?: string;
}

export function PandaViewer({ size = 110, className = "" }: PandaViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [loaded, setLoaded] = useState(false);
  const [hasError, setHasError] = useState(false);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let animId: number | null = null;
    let model: THREE.Group | null = null;
    let isDisposed = false;

    // 1. Configurar Cena e Câmera
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(38, 1, 0.1, 100);
    camera.position.set(0, 1.2, 3.8);

    // 2. Configurar Renderizador com fundo transparente
    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({
        antialias: true,
        alpha: true,
        powerPreference: "low-power",
      });
      renderer.setSize(size, size);
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
      renderer.outputColorSpace = THREE.SRGBColorSpace;
      container.appendChild(renderer.domElement);
    } catch {
      setHasError(true);
      return;
    }

    // 3. Iluminação sutil investigativa
    const ambientLight = new THREE.AmbientLight(0xffffff, 1.8);
    scene.add(ambientLight);

    const dirLight = new THREE.DirectionalLight(0xfff4e0, 2.2);
    dirLight.position.set(2, 4, 3);
    scene.add(dirLight);

    const rimLight = new THREE.DirectionalLight(0xd99b26, 1.2);
    rimLight.position.set(-3, 2, -2);
    scene.add(rimLight);

    // 4. Carregar Panda .glb local
    const loader = new GLTFLoader();
    loader.load(
      "/panda.glb",
      (gltf) => {
        if (isDisposed) return;
        model = gltf.scene;

        // Centralizar e ajustar escala do modelo
        const box = new THREE.Box3().setFromObject(model);
        const center = box.getCenter(new THREE.Vector3());
        const boxSize = box.getSize(new THREE.Vector3());
        const maxDim = Math.max(boxSize.x, boxSize.y, boxSize.z);
        const scale = 1.85 / (maxDim || 1);

        model.scale.setScalar(scale);
        model.position.set(-center.x * scale, -center.y * scale - 0.15, -center.z * scale);

        scene.add(model);
        setLoaded(true);
      },
      undefined,
      () => {
        if (!isDisposed) setHasError(true);
      },
    );

    // 5. Interação suave com o mouse
    let mouseX = 0;
    let targetRotationY = 0;

    const handleMouseMove = (e: MouseEvent) => {
      const rect = container.getBoundingClientRect();
      const x = (e.clientX - rect.left) / rect.width - 0.5;
      mouseX = x * 1.5;
    };

    const handleMouseLeave = () => {
      mouseX = 0;
    };

    container.addEventListener("mousemove", handleMouseMove);
    container.addEventListener("mouseleave", handleMouseLeave);

    // 6. Loop de Animação suave
    let clock = 0;
    const animate = () => {
      animId = requestAnimationFrame(animate);

      if (model) {
        clock += 0.012;
        // Rotação suave idle combinada com resposta do cursor
        targetRotationY = clock + mouseX;
        model.rotation.y += (targetRotationY - model.rotation.y) * 0.08;
        // Leve inclinação para dar vida ao personagem
        model.rotation.x = Math.sin(clock * 0.8) * 0.04;
      }

      renderer.render(scene, camera);
    };

    animate();

    // Limpeza de recursos na desmontagem
    return () => {
      isDisposed = true;
      if (animId) cancelAnimationFrame(animId);

      container.removeEventListener("mousemove", handleMouseMove);
      container.removeEventListener("mouseleave", handleMouseLeave);

      scene.clear();
      renderer.dispose();
      if (renderer.domElement.parentElement) {
        renderer.domElement.parentElement.removeChild(renderer.domElement);
      }
    };
  }, [size]);

  if (hasError) {
    return (
      <div
        className={`home__mascot-avatar ${className}`}
        style={{ width: `${size}px`, height: `${size}px`, fontSize: `${Math.floor(size * 0.45)}px` }}
        title="Panda Investigador · Asset Poly Pizza / Quaternius"
      >
        <span>🐼</span>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className={`home__mascot-3d ${className}`}
      style={{
        width: `${size}px`,
        height: `${size}px`,
        position: "relative",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        cursor: "grab",
      }}
      title="Panda Investigador 3D (Poly Pizza / Quaternius) · Arraste ou passe o cursor"
    >
      {!loaded && (
        <span style={{ fontSize: `${Math.floor(size * 0.4)}px`, opacity: 0.6 }}>
          🐼
        </span>
      )}
    </div>
  );
}
