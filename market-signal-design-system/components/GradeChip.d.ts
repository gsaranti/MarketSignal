import type { CSSProperties, ReactNode } from "react";

export interface GradeChipProps {
  /** Grade letter — A, B, C, D, or F. May include a modifier like "A−". */
  value?: ReactNode;
  /** Chip size. Default "md". */
  size?: "sm" | "md" | "lg";
  style?: CSSProperties;
}

/** A discrete tonal grade chip (A–F) from the unified analytical palette. */
export function GradeChip(props: GradeChipProps): JSX.Element;
