import { Subcomponent } from "@anvil-di/anvil";
import { RequestHandler } from "./request-handler";

@Subcomponent({ modules: [] })
export abstract class RequestComponent {
  abstract handler(): RequestHandler;
}
