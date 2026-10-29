import { Database } from "lucide-react";
import aws from "@/assets/data-sources/aws.svg";
import azure from "@/assets/data-sources/azure.svg";
import googleCloud from "@/assets/data-sources/google-cloud.svg";
import mariadb from "@/assets/data-sources/mariadb.svg";
import mysql from "@/assets/data-sources/mysql.svg";
import postgresql from "@/assets/data-sources/postgresql.svg";
import sqlite from "@/assets/data-sources/sqlite.svg";
import sqlserver from "@/assets/data-sources/sqlserver.svg";
import { cn } from "@/lib/cn";

/** Logo per `DataSource.id`. A source the engine adds before it has a logo gets a plain icon. */
const LOGOS: Record<string, string> = {
  mysql,
  mariadb,
  postgresql,
  sqlserver,
  "google-cloud": googleCloud,
  aws,
  azure,
  "demo-postgresql": postgresql,
  "demo-mysql": mysql,
  "demo-sqlite": sqlite,
};

const SIZES = {
  sm: { box: "size-5 rounded p-0.5", icon: 12 },
  lg: { box: "size-12 rounded-lg p-2", icon: 24 },
} as const;

export function DataSourceLogo({ id, size = "lg" }: { id: string; size?: keyof typeof SIZES }) {
  const logo = LOGOS[id];
  const { box, icon } = SIZES[size];
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center justify-center",
        box,
        logo ? "bg-logo-plate" : "bg-surface text-muted",
      )}
    >
      {logo ? (
        <img src={logo} alt="" className="size-full object-contain" draggable={false} />
      ) : (
        <Database size={icon} strokeWidth={1.75} />
      )}
    </span>
  );
}
