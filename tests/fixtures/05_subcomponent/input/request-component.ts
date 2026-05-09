import { Subcomponent } from "@msulak/anvil";
import { RequestHandler } from "./request-handler";

@Subcomponent({ modules: [] })
export abstract class RequestComponent {
  abstract handler(): RequestHandler;
}
