import { Subcomponent } from "tsdi";
import { RequestHandler } from "./request-handler";

@Subcomponent({ modules: [] })
export abstract class RequestComponent {
  abstract handler(): RequestHandler;
}
