import type { Environment } from "@/bindings/Environment";
import { cn } from "@/lib/cn";

/** Staging/production badge. Local connections show nothing. Always text, never color alone. */
export function EnvBadge({
  environment,
  className,
}: {
  environment: Environment;
  className?: string;
}) {
  if (environment === "local") return null;
  return (
    <span
      className={cn(
        "inline-flex h-4 shrink-0 items-center rounded-sm px-1 font-semibold text-xs leading-none tracking-wide",
        environment === "production"
          ? "bg-env-production text-white"
          : "bg-env-staging text-black/80",
        className,
      )}
    >
      {environment === "production" ? "PROD" : "STAGING"}
    </span>
  );
}
