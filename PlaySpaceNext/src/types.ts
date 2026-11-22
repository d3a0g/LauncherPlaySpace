export interface Game {id:string;name:string;exePath:string;steamAppId:string;isPinned:boolean;playStatus?:string;playtime:string;playedSeconds:number;lastLaunchUtc:string|null;source:string;achievementSummary:string|null;customArtwork?:string|null}
export interface Tool {id:string;title:string;description:string;group:string;requiresGame:boolean;engine:'native'|'compatibility'}
export interface Bootstrap {games:Game[];legacyAvailable:boolean;tools:Tool[];profileFolder:string;notice?:string|null}
export interface Recommendation {id:string;name:string;categories:string[]}
