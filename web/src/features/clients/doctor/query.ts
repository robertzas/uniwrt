import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { api } from "../../../lib/api";
import { DOCTOR_POLL_MS, type DoctorHours } from "./describe";

/** The client's diagnosis over `hours`, read again each minute; the tab and the chip share it. */
export function createDiagnosis(mac: () => string, hours: () => DoctorHours) {
  return useQuery(() => ({
    queryKey: ["doctor", mac(), hours()],
    queryFn: ({ signal }) => api.clients.doctor(mac(), hours(), signal),
    refetchInterval: DOCTOR_POLL_MS,
    placeholderData: keepPreviousData,
  }));
}
