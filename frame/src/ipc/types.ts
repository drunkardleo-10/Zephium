export interface TabView {
  id: string;
  title: string;
  url: string | null;
  loading: boolean;
  can_go_back: boolean;
  can_go_forward: boolean;
}

export interface TabsSnapshot {
  tabs: TabView[];
  active: string | null;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}
