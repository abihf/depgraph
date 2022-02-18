export type Dependency = {
  k: number, 
  n: string, 
  d: number, 
  l: number, 
  c: number
}
export type ErrorString = string;
export type FileName = string;
export type Item = [FileName, ErrorString | Dependency];
